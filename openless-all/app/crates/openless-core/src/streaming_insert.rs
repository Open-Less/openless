//! 流式插入的纯策略与 Unicode 边界规则。

use crate::shared_types::{ChineseScriptPreference, MacosNewlineMode};

pub const STREAMING_FLUSH_INTERVAL_MS: u64 = 12;

const TERMINAL_BUNDLE_PREFIXES: &[&str] = &[
    "com.apple.terminal",
    "com.googlecode.iterm2",
    "dev.warp.warp",
    "com.github.wez.wezterm",
    "io.alacritty",
    "org.alacritty",
    "net.kovidgoyal.kitty",
    "co.zeit.hyper",
    "org.tabby",
    "com.tabby",
    "com.mitchellh.ghostty",
];

pub fn resolve_macos_newline_mode(
    configured: MacosNewlineMode,
    front_app: Option<&str>,
) -> MacosNewlineMode {
    if configured != MacosNewlineMode::Auto {
        return configured;
    }
    let front = front_app.map(|label| crate::shared_types::split_front_app_label(label, true));
    let identity = front
        .as_ref()
        .and_then(|front| front.bundle_id.as_deref().or(front.name.as_deref()));
    if identity.is_some_and(|value| {
        let value = value.to_ascii_lowercase();
        TERMINAL_BUNDLE_PREFIXES
            .iter()
            .any(|prefix| value.starts_with(prefix))
    }) {
        MacosNewlineMode::LineFeed
    } else {
        MacosNewlineMode::ShiftReturn
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamingInsertState {
    pub pending: String,
    pub typed_text: String,
    pub failed: Option<String>,
    accepted_chars: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalReconciliation {
    InsertFinal(String),
    WriteTail(String),
    CopyFallback(String),
    Complete,
}

impl StreamingInsertState {
    pub fn reconcile_final(&self, final_text: &str) -> FinalReconciliation {
        if self.failed.is_some() {
            return FinalReconciliation::CopyFallback(final_text.to_string());
        }
        if self.typed_text.is_empty() {
            return FinalReconciliation::InsertFinal(final_text.to_string());
        }
        match final_text.strip_prefix(&self.typed_text) {
            Some("") => FinalReconciliation::Complete,
            Some(tail) => FinalReconciliation::WriteTail(tail.to_string()),
            None => FinalReconciliation::CopyFallback(final_text.to_string()),
        }
    }

    pub fn push_delta(&mut self, offset: u64, delta: &str) {
        if self.failed.is_some() || delta.is_empty() {
            return;
        }
        if offset > self.accepted_chars {
            self.failed = Some(format!(
                "polish delta skipped from {} to {offset}",
                self.accepted_chars
            ));
            return;
        }
        let overlap = self.accepted_chars.saturating_sub(offset) as usize;
        let suffix = delta.chars().skip(overlap).collect::<String>();
        self.accepted_chars = self
            .accepted_chars
            .saturating_add(suffix.chars().count() as u64);
        self.pending.push_str(&suffix);
    }

    /// Flushes pending text through the host inserter. A partial Unicode write
    /// is retained as a typed prefix and becomes an explicit fallback.
    pub fn flush<F>(&mut self, mut insert: F) -> Result<usize, String>
    where
        F: FnMut(&str) -> Result<usize, String>,
    {
        if self.failed.is_some() || self.pending.is_empty() {
            return Ok(0);
        }
        let delta = std::mem::take(&mut self.pending);
        let expected = delta.chars().count();
        match insert(&delta) {
            Ok(typed) if typed >= expected => {
                self.typed_text.push_str(&delta);
                Ok(expected)
            }
            Ok(typed) => {
                let appended = append_typed_prefix(&mut self.typed_text, &delta, typed);
                self.failed = Some(format!(
                    "host inserted only {appended}/{expected} characters"
                ));
                Ok(appended)
            }
            Err(error) => {
                self.failed = Some(error.clone());
                Err(error)
            }
        }
    }
}

pub fn apply_chinese_script_preference(text: &str, preference: ChineseScriptPreference) -> String {
    use ferrous_opencc::config::BuiltinConfig;

    let config = match preference {
        ChineseScriptPreference::Simplified => Some(BuiltinConfig::T2s),
        ChineseScriptPreference::Traditional => Some(BuiltinConfig::S2t),
        ChineseScriptPreference::Auto => None,
    };
    config
        .and_then(|config| ferrous_opencc::OpenCC::from_config(config).ok())
        .map_or_else(|| text.to_string(), |converter| converter.convert(text))
}

/// 剝離模型完整回顯的 user-message 腳手架。
///
/// 以真正的 prompt builder 產生 canonical 模板，再比對標籤前後的完整內容；不手抄
/// 導語，避免 prompt 改字後判定漂移。另接受整套模板被簡繁轉換後的版本。只有完整
/// 模板命中才取信封正文；單純 XML、文件範例或前後另有正文一律保留原輸出。
fn strip_complete_template(text: &str, template: &str, open: &str, close: &str) -> Option<String> {
    let template_open = template.find(open)?;
    let template_inner_start = template_open + open.len();
    let template_close = template[template_inner_start..].find(close)? + template_inner_start;
    let expected_before = normalize_scaffold_prose(template[..template_open].trim());
    let expected_after = normalize_scaffold_prose(template[template_close + close.len()..].trim());

    let trimmed = text.trim();
    let open_pos = trimmed.find(open)?;
    let inner_start = open_pos + open.len();
    let inner_end = trimmed[inner_start..].find(close)? + inner_start;
    let before = normalize_scaffold_prose(trimmed[..open_pos].trim());
    let after = normalize_scaffold_prose(trimmed[inner_end + close.len()..].trim());
    if before != expected_before || after != expected_after {
        return None;
    }
    let inner = trimmed[inner_start..inner_end].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

fn normalize_scaffold_prose(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn strip_echoed_scaffolding(text: &str) -> String {
    const MARKER: &str = "__OPENLESS_SCAFFOLD_BODY__";
    let raw = crate::prompts::user_prompt(MARKER);
    let selection = crate::prompts::selection_user_prompt(MARKER);
    let raw_traditional = apply_chinese_script_preference(
        &raw,
        ChineseScriptPreference::Traditional,
    );
    let selection_traditional = apply_chinese_script_preference(
        &selection,
        ChineseScriptPreference::Traditional,
    );

    let stripped = [raw.as_str(), raw_traditional.as_str()]
        .into_iter()
        .find_map(|template| {
            strip_complete_template(text, template, "<raw_transcript>", "</raw_transcript>")
        })
        .or_else(|| {
            [selection.as_str(), selection_traditional.as_str()]
                .into_iter()
                .find_map(|template| {
                    strip_complete_template(
                        text,
                        template,
                        "<selected_text>",
                        "</selected_text>",
                    )
                })
        })
        .unwrap_or_else(|| text.to_string());
    stripped
}

pub fn append_typed_prefix(target: &mut String, delta: &str, typed_chars: usize) -> usize {
    let prefix: String = delta.chars().take(typed_chars).collect();
    let count = prefix.chars().count();
    target.push_str(&prefix);
    count
}

pub fn streaming_insert_eligible(
    enabled: bool,
    _translation_active: bool,
    traditional_script: bool,
    windows_paste_insertion: bool,
) -> bool {
    // Translation providers now emit only target-language deltas, so they share
    // the same native insertion constraints as ordinary polishing.
    enabled && !traditional_script && !windows_paste_insertion
}

#[cfg(test)]
mod tests {
    #[test]
    fn strip_echoed_scaffolding_none_passes_through() {
        assert_eq!(strip_echoed_scaffolding("純正文，沒有標籤。"), "純正文，沒有標籤。");
    }

    #[test]
    fn strip_echoed_scaffolding_unescaped_user_content_not_touched() {
        // 用戶正文裡的標籤經 sanitize 後是 &lt;raw_transcript，不是活標籤——不得剝。
        let text = "文中提到 &lt;raw_transcript 的用法。";
        assert_eq!(strip_echoed_scaffolding(text), text);
    }

    #[test]
    fn strip_echoed_scaffolding_drops_echoed_template_and_tags() {
        // 蜘蛛故事事故形態：模型照抄整套 raw user prompt，並被轉成繁體。
        let text = crate::prompts::user_prompt("從前，有一隻蜘蛛。");
        let text = apply_chinese_script_preference(&text, ChineseScriptPreference::Traditional);
        assert_eq!(strip_echoed_scaffolding(&text), "從前，有一隻蜘蛛。");
    }

    #[test]
    fn strip_echoed_scaffolding_empty_inner_keeps_original() {
        // 標籤內為空（模型只回顯了空信封）——保守不剝，避免把正文吃掉。
        let text = "脚手架\n<raw_transcript>\n</raw_transcript>\n正文在這裡。";
        assert_eq!(strip_echoed_scaffolding(text), text);
    }

    #[test]
    fn strip_echoed_scaffolding_missing_close_keeps_original() {
        // 流被截斷、只回顯了開標籤——保守不剝。
        let text = "<raw_transcript>\n只有開標籤，流斷了。";
        assert_eq!(strip_echoed_scaffolding(text), text);
    }

    #[test]
    fn strip_echoed_scaffolding_legitimate_xml_is_not_touched() {
        let text = "請輸出以下 XML 範例：<selected_text>保留我</selected_text>，並補充說明。";
        assert_eq!(strip_echoed_scaffolding(text), text);
    }

    #[test]
    fn strip_echoed_scaffolding_selected_text_echo_stripped() {
        // 圈選路徑：模型完整回顯真實 selection user prompt。
        let text = crate::prompts::selection_user_prompt("从前，有一只蜘蛛。");
        assert_eq!(strip_echoed_scaffolding(&text), "从前，有一只蜘蛛。");
    }

    #[test]
    fn strip_echoed_scaffolding_selected_text_empty_inner_keeps_original() {
        let text = "脚手架\n<selected_text>\n</selected_text>\n正文在这里。";
        assert_eq!(strip_echoed_scaffolding(text), text);
    }

    #[test]
    fn strip_echoed_scaffolding_escaped_selected_text_not_touched() {
        // 用戶正文裡的 <selected_text> 經 sanitize 後是 &lt;selected_text，不是活標籤。
        let text = "文中提到 &lt;selected_text 的用法。";
        assert_eq!(strip_echoed_scaffolding(text), text);
    }

    use super::*;
    use crate::shared_types::MacosNewlineMode;

    #[test]
    fn macos_auto_newline_uses_line_feed_only_for_known_terminals() {
        assert_eq!(
            resolve_macos_newline_mode(
                MacosNewlineMode::Auto,
                Some("Terminal (com.apple.Terminal)")
            ),
            MacosNewlineMode::LineFeed
        );
        assert_eq!(
            resolve_macos_newline_mode(MacosNewlineMode::Auto, Some("Chat")),
            MacosNewlineMode::ShiftReturn
        );
        assert_eq!(
            resolve_macos_newline_mode(MacosNewlineMode::Return, None),
            MacosNewlineMode::Return
        );
    }

    #[test]
    fn final_reconciliation_never_retypes_an_accepted_prefix() {
        let mut stream = StreamingInsertState::default();
        assert_eq!(
            stream.reconcile_final("你好"),
            FinalReconciliation::InsertFinal("你好".into())
        );

        stream.typed_text = "你".into();
        assert_eq!(
            stream.reconcile_final("你好"),
            FinalReconciliation::WriteTail("好".into())
        );
        assert_eq!(stream.reconcile_final("你"), FinalReconciliation::Complete);

        stream.typed_text = "旧".into();
        assert_eq!(
            stream.reconcile_final("新文本"),
            FinalReconciliation::CopyFallback("新文本".into())
        );
    }

    #[test]
    fn failed_stream_preserves_the_complete_final_for_fallback() {
        let stream = StreamingInsertState {
            typed_text: "已经".into(),
            failed: Some("partial write".into()),
            ..StreamingInsertState::default()
        };
        assert_eq!(
            stream.reconcile_final("已经完成"),
            FinalReconciliation::CopyFallback("已经完成".into())
        );
    }

    #[test]
    fn partial_unicode_write_is_explicit_and_prefix_safe() {
        let mut state = StreamingInsertState::default();
        state.push_delta(0, "你好🙂");
        let written = state.flush(|_| Ok(2)).unwrap();
        assert_eq!(written, 2);
        assert_eq!(state.typed_text, "你好");
        assert!(state.failed.is_some());
    }

    #[test]
    fn duplicate_and_out_of_order_deltas_are_not_typed_twice() {
        let mut state = StreamingInsertState::default();
        state.push_delta(0, "你好");
        state.push_delta(0, "你好");
        state.push_delta(3, "跳");
        assert_eq!(state.pending, "你好");
        assert!(state.failed.is_some());
    }

    #[test]
    fn policy_blocks_only_unsafe_modes() {
        assert!(streaming_insert_eligible(true, false, false, false));
        assert!(streaming_insert_eligible(true, true, false, false));
        assert!(!streaming_insert_eligible(true, false, true, false));
    }
}
