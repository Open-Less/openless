//! Platform-independent document-window and vocabulary-learning rules.
//!
//! AX/IME/clipboard reads stay in the host; the Core only provides testable
//! pure functions.

mod diff;
mod observation;
mod window;

pub use diff::{
    edit_is_within_typed_text, is_vocab_worthy, learned_rule, learned_rule_with_max_chars,
    minimal_edit, EditPair, LearnedRule,
};
pub use observation::ObservedInsertion;
pub use window::{
    plan_window, utf16_offset_to_char_offset, window_around_cursor, window_from_split, WindowSpan,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentWindow {
    pub text: String,
    pub cursor: usize,
}

impl DocumentWindow {
    pub fn before(&self) -> &str {
        let index = self
            .text
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len());
        &self.text[..index]
    }

    pub fn after(&self) -> &str {
        let index = self
            .text
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len());
        &self.text[index..]
    }
}
