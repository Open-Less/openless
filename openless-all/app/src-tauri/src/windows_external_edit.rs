use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationValuePattern, UIA_TextPatternId, UIA_ValuePatternId,
};

const BASELINE_DELAY: Duration = Duration::from_millis(180);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const OBSERVATION_WINDOW: Duration = Duration::from_secs(8);
const MAX_LEARNED_SPAN_CHARS: usize = 48;
const MAX_LOG_PREVIEW_CHARS: usize = 120;

#[derive(Debug, Clone)]
pub struct ExternalEditObservationRequest {
    pub inserted_text: String,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedCorrection {
    pub pattern: String,
    pub replacement: String,
    pub baseline_text: String,
    pub observed_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalEditObservationOutcome {
    Learned(LearnedCorrection),
    NoChange,
    Skipped(&'static str),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
struct TextSnapshot {
    text: String,
    class_name: String,
    name: String,
}

pub struct WindowsExternalEditObserver {
    generation: Arc<AtomicU64>,
}

impl Default for WindowsExternalEditObserver {
    fn default() -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl WindowsExternalEditObserver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn arm<F>(&self, request: ExternalEditObservationRequest, on_outcome: F)
    where
        F: FnOnce(ExternalEditObservationOutcome) + Send + 'static,
    {
        let generation_state = Arc::clone(&self.generation);
        let generation = generation_state.fetch_add(1, Ordering::SeqCst) + 1;
        thread::spawn(move || {
            let mut callback = Some(on_outcome);

            thread::sleep(BASELINE_DELAY);
            if Self::is_cancelled(&generation_state, generation) {
                if let Some(cb) = callback.take() {
                    cb(ExternalEditObservationOutcome::Cancelled);
                }
                return;
            }

            let baseline = match snapshot_external_edit_text() {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) => {
                    if let Some(cb) = callback.take() {
                        cb(ExternalEditObservationOutcome::Skipped("baselineUnavailable"));
                    }
                    return;
                }
                Err(err) => {
                    if let Some(cb) = callback.take() {
                        cb(ExternalEditObservationOutcome::Failed(format!(
                            "baseline capture failed: {err}"
                        )));
                    }
                    return;
                }
            };

            if !request.inserted_text.is_empty()
                && !has_unique_occurrence(&baseline.text, &request.inserted_text)
            {
                let occurrences = occurrence_count(&baseline.text, &request.inserted_text);
                log::info!(
                    "[extedit] baseline rejected: reason=insertedTextNotUniqueInBaseline occurrences={} baseline_chars={} inserted_chars={} class={:?} name={:?} expected_window={:?} preview={:?}",
                    occurrences,
                    baseline.text.chars().count(),
                    request.inserted_text.chars().count(),
                    baseline.class_name,
                    baseline.name,
                    request.window_title,
                    preview_text(&baseline.text),
                );
                if let Some(cb) = callback.take() {
                    cb(ExternalEditObservationOutcome::Skipped(
                        "insertedTextNotUniqueInBaseline",
                    ));
                }
                return;
            }

            let deadline = Instant::now() + OBSERVATION_WINDOW;
            while Instant::now() < deadline {
                if Self::is_cancelled(&generation_state, generation) {
                    if let Some(cb) = callback.take() {
                        cb(ExternalEditObservationOutcome::Cancelled);
                    }
                    return;
                }
                thread::sleep(POLL_INTERVAL);
                let observed = match snapshot_external_edit_text() {
                    Ok(Some(snapshot)) => snapshot,
                    Ok(None) => continue,
                    Err(err) => {
                        if let Some(cb) = callback.take() {
                            cb(ExternalEditObservationOutcome::Failed(format!(
                                "poll capture failed: {err}"
                            )));
                        }
                        return;
                    }
                };
                if observed.text == baseline.text {
                    continue;
                }
                match infer_learned_correction(
                    &baseline.text,
                    &observed.text,
                    &request.inserted_text,
                ) {
                    Some(rule) => {
                        if let Some(cb) = callback.take() {
                            cb(ExternalEditObservationOutcome::Learned(rule));
                        }
                        return;
                    }
                    None => {
                        if let Some(cb) = callback.take() {
                            cb(ExternalEditObservationOutcome::Skipped(
                                "deterministicInferenceFailed",
                            ));
                        }
                        return;
                    }
                }
            }

            if let Some(cb) = callback.take() {
                cb(ExternalEditObservationOutcome::NoChange);
            }
        });
    }

    fn is_cancelled(generation_state: &AtomicU64, generation: u64) -> bool {
        generation != generation_state.load(Ordering::SeqCst)
    }
}

fn has_unique_occurrence(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut matches = haystack.match_indices(needle);
    let first = matches.next();
    first.is_some() && matches.next().is_none()
}

fn occurrence_count(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.match_indices(needle).count()
}

fn preview_text(text: &str) -> String {
    let preview: String = text.chars().take(MAX_LOG_PREVIEW_CHARS).collect();
    preview.replace('\r', "\\r").replace('\n', "\\n")
}

fn snapshot_external_edit_text() -> Result<Option<TextSnapshot>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        Ok(None)
    }

    #[cfg(target_os = "windows")]
    {
        let com_initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let should_uninitialize = if com_initialized.is_ok() {
            true
        } else if com_initialized == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(format!("CoInitializeEx: {com_initialized}"));
        };
        let result = snapshot_external_edit_text_windows();
        if should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn snapshot_external_edit_text_windows() -> Result<Option<TextSnapshot>, String> {
    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
            .map_err(|err| format!("CoCreateInstance(CUIAutomation8): {err}"))?
    };
    let element = unsafe {
        automation
            .GetFocusedElement()
            .map_err(|err| format!("GetFocusedElement: {err}"))?
    };
    read_text_from_element(&element)
}

#[cfg(target_os = "windows")]
fn read_text_from_element(element: &IUIAutomationElement) -> Result<Option<TextSnapshot>, String> {
    let class_name = unsafe {
        element
            .CurrentClassName()
            .map(|value| value.to_string())
            .unwrap_or_default()
    };
    let name = unsafe {
        element
            .CurrentName()
            .map(|value| value.to_string())
            .unwrap_or_default()
    };

    if let Ok(value_pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    {
        let value = unsafe {
            value_pattern
                .CurrentValue()
                .map_err(|err| format!("CurrentValue: {err}"))?
        };
        let text = value.to_string();
        if !text.is_empty() {
            return Ok(Some(TextSnapshot {
                text,
                class_name,
                name,
            }));
        }
    }

    if let Ok(text_pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
    {
        let document_range = unsafe {
            text_pattern
                .DocumentRange()
                .map_err(|err| format!("DocumentRange: {err}"))?
        };
        let text = unsafe {
            document_range
                .GetText(-1)
                .map_err(|err| format!("GetText: {err}"))?
        }
        .to_string();
        if !text.is_empty() {
            return Ok(Some(TextSnapshot {
                text,
                class_name,
                name,
            }));
        }
    }

    Ok(None)
}

pub fn infer_learned_correction(
    baseline: &str,
    observed: &str,
    inserted_text: &str,
) -> Option<LearnedCorrection> {
    if baseline == observed {
        return None;
    }
    if !inserted_text.is_empty() && !has_unique_occurrence(baseline, inserted_text) {
        return None;
    }

    let baseline_chars: Vec<char> = baseline.chars().collect();
    let observed_chars: Vec<char> = observed.chars().collect();
    let mut prefix = 0usize;
    while prefix < baseline_chars.len()
        && prefix < observed_chars.len()
        && baseline_chars[prefix] == observed_chars[prefix]
    {
        prefix += 1;
    }

    let mut baseline_suffix = baseline_chars.len();
    let mut observed_suffix = observed_chars.len();
    while baseline_suffix > prefix
        && observed_suffix > prefix
        && baseline_chars[baseline_suffix - 1] == observed_chars[observed_suffix - 1]
    {
        baseline_suffix -= 1;
        observed_suffix -= 1;
    }

    let old_span: String = baseline_chars[prefix..baseline_suffix].iter().collect();
    let new_span: String = observed_chars[prefix..observed_suffix].iter().collect();
    if old_span.trim().is_empty() || new_span.trim().is_empty() || old_span == new_span {
        return None;
    }
    if old_span.contains('\n') || new_span.contains('\n') {
        return None;
    }
    if old_span.chars().count() > MAX_LEARNED_SPAN_CHARS
        || new_span.chars().count() > MAX_LEARNED_SPAN_CHARS
    {
        return None;
    }

    if !inserted_text.is_empty() {
        let (insert_start, insert_end) = unique_match_range(baseline, inserted_text)?;
        let diff_start = char_to_byte_index(baseline, prefix)?;
        let diff_end = char_to_byte_index(baseline, baseline_suffix)?;
        if diff_start >= insert_end || diff_end <= insert_start {
            return None;
        }
    }

    let (pattern, replacement) = generalize_numeric_pair(&old_span, &new_span)
        .unwrap_or_else(|| (old_span.clone(), new_span.clone()));

    Some(LearnedCorrection {
        pattern,
        replacement,
        baseline_text: baseline.to_string(),
        observed_text: observed.to_string(),
    })
}

fn unique_match_range(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    let mut matches = haystack.match_indices(needle);
    let (start, matched) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some((start, start + matched.len()))
}

fn char_to_byte_index(text: &str, char_index: usize) -> Option<usize> {
    if char_index == text.chars().count() {
        return Some(text.len());
    }
    text.char_indices().nth(char_index).map(|(index, _)| index)
}

fn generalize_numeric_pair(old_span: &str, new_span: &str) -> Option<(String, String)> {
    let old_token = single_ascii_digit_run(old_span)?;
    let new_token = single_ascii_digit_run(new_span)?;
    if old_token.text != new_token.text {
        return None;
    }
    let old_pattern = replace_range_with_num(old_span, old_token.start, old_token.end)?;
    let new_pattern = replace_range_with_num(new_span, new_token.start, new_token.end)?;
    if old_pattern == "{num}" || new_pattern == "{num}" {
        return None;
    }
    Some((old_pattern, new_pattern))
}

struct DigitRun<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn single_ascii_digit_run(text: &str) -> Option<DigitRun<'_>> {
    let bytes = text.as_bytes();
    let mut run_start: Option<usize> = None;
    let mut run_end = 0usize;
    let mut seen = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte.is_ascii_digit() {
            if run_start.is_none() {
                if seen {
                    return None;
                }
                run_start = Some(index);
            }
            run_end = index + 1;
        } else if run_start.is_some() {
            seen = true;
        }
    }
    let start = run_start?;
    Some(DigitRun {
        start,
        end: run_end,
        text: &text[start..run_end],
    })
}

fn replace_range_with_num(text: &str, start: usize, end: usize) -> Option<String> {
    let prefix = text.get(..start)?;
    let suffix = text.get(end..)?;
    Some(format!("{prefix}{{num}}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::{infer_learned_correction, LearnedCorrection};

    fn learned(
        pattern: &str,
        replacement: &str,
        baseline: &str,
        observed: &str,
    ) -> LearnedCorrection {
        LearnedCorrection {
            pattern: pattern.into(),
            replacement: replacement.into(),
            baseline_text: baseline.into(),
            observed_text: observed.into(),
        }
    }

    #[test]
    fn infers_literal_replacement_inside_inserted_span() {
        let baseline = "今天记录了一个几粒样本。";
        let observed = "今天记录了一个几例样本。";
        let inserted = "今天记录了一个几粒样本。";

        assert_eq!(
            infer_learned_correction(baseline, observed, inserted),
            Some(learned("几粒", "几例", baseline, observed))
        );
    }

    #[test]
    fn infers_numeric_generalization_when_number_matches() {
        let baseline = "本轮共统计 2粒 阳性。";
        let observed = "本轮共统计 2例 阳性。";
        let inserted = baseline;

        assert_eq!(
            infer_learned_correction(baseline, observed, inserted),
            Some(learned("{num}粒", "{num}例", baseline, observed))
        );
    }

    #[test]
    fn rejects_diff_outside_inserted_span() {
        let baseline = "前缀A 几粒 后缀";
        let observed = "前缀B 几粒 后缀";

        assert_eq!(infer_learned_correction(baseline, observed, "几粒"), None);
    }

    #[test]
    fn rejects_ambiguous_inserted_occurrence() {
        let baseline = "几粒 和 几粒";
        let observed = "几例 和 几粒";

        assert_eq!(infer_learned_correction(baseline, observed, "几粒"), None);
    }
}
