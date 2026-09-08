//! 与平台无关的文档窗口和词汇学习规则。
//!
//! AX/IME/clipboard 读取仍由宿主实现；Core 只提供可测试的纯函数。

mod diff;
mod window;

pub use diff::{
    edit_is_within_typed_text, is_vocab_worthy, learned_rule, minimal_edit, EditPair, LearnedRule,
};
pub use window::{plan_window, utf16_offset_to_char_offset, window_around_cursor, WindowSpan};

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
