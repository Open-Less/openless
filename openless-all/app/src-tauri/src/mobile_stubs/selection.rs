//! Mobile stub — selection capture is desktop-only for now.

#[derive(Debug, Clone)]
pub struct SelectionContext {
    pub text: String,
    pub source_app: Option<String>,
}

pub fn capture_selection() -> Option<SelectionContext> {
    None
}
