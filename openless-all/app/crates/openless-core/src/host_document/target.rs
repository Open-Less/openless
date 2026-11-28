//! Local target evidence shared by host integrations. Never include these tokens
//! in prompts or history: they identify a destination, not document context.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTargetIdentity {
    pub process_id: u32,
    /// Opaque, process-local token; not a native handle or a window title.
    pub window_token: String,
    /// Opaque token for the actual focused input control, not merely its app.
    pub control_token: String,
    pub selection: Option<HostSelectionRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSelectionRange {
    pub start: u64,
    pub length: u64,
    /// Offsets are native units. Consumers must not slice Rust strings using
    /// these values (AX/Win32 use UTF-16; UIA may use provider text units).
    pub unit: HostTextUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTextUnit {
    Utf16,
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTargetValidation {
    Current,
    Unavailable,
    TargetChanged,
    PositionChanged,
    Cancelled,
}
