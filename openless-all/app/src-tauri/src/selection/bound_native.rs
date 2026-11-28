//! A native transaction operates only on the immutable objects in its binding.
//! Foreground checks can reject the operation, but can never supply its object.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextRange {
    pub start: u64,
    pub length: u64,
}

pub(crate) struct Binding<C, W> {
    pub control: C,
    pub window: W,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    Unavailable,
    TargetChanged,
    SelectionChanged,
    /// Submission has happened. Neither retries nor "nothing was written" are safe.
    WriteUnconfirmed,
}

impl Failure {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Unavailable => "selectionPolishTargetUnavailable",
            Self::TargetChanged => "selectionPolishTargetChanged",
            Self::SelectionChanged => "selectionPolishSelectionChanged",
            Self::WriteUnconfirmed => "selectionPolishWriteUnconfirmed",
        }
    }
}

pub(crate) trait Native {
    type Control;
    type Window;
    fn guard(&self, control: &Self::Control, window: &Self::Window) -> Result<(), Failure>;
    fn selected_range(&self, control: &Self::Control) -> Result<TextRange, Failure>;
    fn text(&self, control: &Self::Control, range: TextRange) -> Result<String, Failure>;
    fn writable(&self, control: &Self::Control) -> Result<(), Failure>;
    fn submit(&self, control: &Self::Control, replacement: &str) -> Result<(), Failure>;
}

pub(crate) fn verify<N: Native>(
    native: &N,
    binding: &Binding<N::Control, N::Window>,
    original: &str,
) -> Result<(), Failure> {
    native.guard(&binding.control, &binding.window)?;
    if native.selected_range(&binding.control)? != binding.range
        || native.text(&binding.control, binding.range)? != original
    {
        return Err(Failure::SelectionChanged);
    }
    native.guard(&binding.control, &binding.window)
}

pub(crate) fn replace<N: Native>(
    native: &N,
    binding: &Binding<N::Control, N::Window>,
    original: &str,
    replacement: &str,
) -> Result<(), Failure> {
    if original.is_empty() || binding.range.length == 0 {
        return Err(Failure::Unavailable);
    }
    replacement_range(binding.range, replacement).ok_or(Failure::Unavailable)?;
    verify(native, binding, original)?;
    native.writable(&binding.control)?;
    verify(native, binding, original)?;
    // From this point every failure has an unknown write outcome. Readback
    // always addresses the frozen control, even if focus changed in submit().
    native
        .submit(&binding.control, replacement)
        .map_err(|_| Failure::WriteUnconfirmed)?;
    confirm_replacement(native, binding, replacement)
}

fn replacement_range(original: TextRange, replacement: &str) -> Option<(TextRange, u64)> {
    let length = u64::try_from(replacement.encode_utf16().count()).ok()?;
    let caret = original.start.checked_add(length)?;
    Some((TextRange { start: original.start, length }, caret))
}

/// A second receipt can resolve an AX setter whose first readback was delayed.
/// It never submits or reads from the newly focused object.
pub(crate) fn confirm_replacement<N: Native>(
    native: &N,
    binding: &Binding<N::Control, N::Window>,
    replacement: &str,
) -> Result<(), Failure> {
    native.guard(&binding.control, &binding.window)
        .map_err(|_| Failure::WriteUnconfirmed)?;
    let (inserted, caret) = replacement_range(binding.range, replacement)
        .ok_or(Failure::WriteUnconfirmed)?;
    let observed = native
        .text(&binding.control, inserted)
        .map_err(|_| Failure::WriteUnconfirmed)?;
    let observed_range = native
        .selected_range(&binding.control)
        .map_err(|_| Failure::WriteUnconfirmed)?;
    if observed != replacement
        || observed_range
            != (TextRange {
                start: caret,
                length: 0,
            })
    {
        return Err(Failure::WriteUnconfirmed);
    }
    native
        .guard(&binding.control, &binding.window)
        .map_err(|_| Failure::WriteUnconfirmed)
}

/// The decoder must not interpret an arbitrary CF object as an AXValue, nor an
/// AXValue of another kind as a CFRange. Native callers release ownership after this call.
pub(crate) fn checked_range<T>(
    is_ax_value: impl FnOnce() -> bool,
    is_cf_range: impl FnOnce() -> bool,
    decode: impl FnOnce() -> Option<T>,
) -> Option<T> {
    if !is_ax_value() || !is_cf_range() {
        return None;
    }
    decode()
}
