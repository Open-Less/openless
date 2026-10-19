//! Localization regression contract (Tauri-free).
//!
//! Guards against *new* raw user-visible Simplified-Chinese string literals
//! sneaking into the Linux egui source outside the translation catalog
//! (`src/i18n.rs`, the one legitimate home for zh-CN source-of-truth text).
//!
//! The `src/ui/shell.rs` module is fully localized, so it is held to a strict
//! zero-CJK rule. Elsewhere, any Simplified-Chinese literal that is not in the
//! checked-in `zh_user_visible_baseline.txt` fails the build; that baseline is
//! refreshed deliberately (see the file header) when a string is intentionally
//! translated or knowingly kept for migration.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use openless_linux_egui::{fmt_l10n, tr_l10n, Lang, LANGS};

#[test]
fn cloud_sync_counts_are_formatted_in_every_language() {
    for lang in LANGS {
        let review = fmt_l10n(lang, "cloudSyncE2ee.deviceReview", &[&2]);
        assert!(!review.contains("{}"), "{lang:?}: {review}");
        assert_eq!(review.matches('2').count(), 1, "{lang:?}: {review}");

        let retention = fmt_l10n(lang, "cloudSyncE2ee.backupRetention", &[&30]);
        assert!(!retention.contains("{}"), "{lang:?}: {retention}");
        assert_eq!(retention.matches("30").count(), 1, "{lang:?}: {retention}");

        let progress = fmt_l10n(lang, "cloudSyncE2ee.conflictProgress", &[&1, &3]);
        let expected = match lang {
            Lang::ZhCn => "已处理 1 / 3 项冲突",
            Lang::ZhTw => "已處理 1 / 3 項衝突",
            Lang::En => "1 of 3 conflicts resolved",
            Lang::Ja => "競合を 1 / 3 件解決済み",
            Lang::Ko => "충돌 1 / 3개 해결",
        };
        assert_eq!(progress, expected, "{lang:?}");
        assert!(!progress.contains("{}"), "{lang:?}: {progress}");
    }
}

#[test]
fn ui_refinement_additions_have_japanese_korean_and_traditional_chinese_translations() {
    const KEYS: [&str; 22] = [
        "uiRefinement.family",
        "uiRefinement.fontHint",
        "uiRefinement.fontSize",
        "uiRefinement.storageError",
        "uiRefinement.typography",
        "uiRefinement.width",
        "uiRefinement.back",
        "uiRefinement.noResults",
        "uiRefinement.shortcutHint",
        "uiRefinement.shortcutsSearch",
        "uiRefinement.unassigned",
        "uiRefinement.all",
        "uiRefinement.assigned",
        "uiRefinement.category",
        "uiRefinement.recording",
        "uiRefinement.status",
        "uiRefinement.styles",
        "uiRefinement.system",
        "uiRefinement.comfortable",
        "uiRefinement.narrow",
        "uiRefinement.native",
        "uiRefinement.wide",
    ];
    for key in KEYS {
        let english = tr_l10n(Lang::En, key);
        for lang in [Lang::Ja, Lang::Ko, Lang::ZhTw] {
            let translated = tr_l10n(lang, key);
            assert!(!translated.trim().is_empty(), "{lang:?}: {key}");
            assert_ne!(translated, key, "{lang:?}: missing {key}");
            if matches!(lang, Lang::Ja | Lang::Ko) {
                assert_ne!(translated, english, "{lang:?}: English fallback for {key}");
            }
        }
    }
    assert_eq!(tr_l10n(Lang::ZhTw, "uiRefinement.family"), "字型系列");
    assert_eq!(tr_l10n(Lang::ZhTw, "uiRefinement.width"), "閱讀寬度");
    assert_eq!(
        tr_l10n(Lang::ZhTw, "uiRefinement.fontHint"),
        "僅使用已安裝的字型，找不到時改用系統字型。不下載、不掃描，也不發出字型網路請求。"
    );
    assert_eq!(
        tr_l10n(Lang::ZhTw, "uiRefinement.shortcutHint"),
        "點選按鍵即可重新錄製。Esc 取消；儲存前會檢查快速鍵衝突。"
    );
}

fn strip_comments(source: &str) -> Vec<u8> {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            // Block / doc-block comment.
            i += 2;
            while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
            continue;
        }
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            // Line / line-doc comment.
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Collect the inner text of every `"..."` string literal (handling `\"` and
/// `\\` escapes) whose content contains any CJK Unified Ideograph.
fn cjk_string_literals(source: &str) -> BTreeSet<String> {
    let cleaned = strip_comments(source);
    let n = cleaned.len();
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i < n {
        if cleaned[i] != b'"' {
            i += 1;
            continue;
        }
        // Inside a string literal: read until an unescaped closing quote.
        let mut inner = Vec::new();
        let mut j = i + 1;
        while j < n {
            if cleaned[j] == b'\\' && j + 1 < n {
                inner.push(cleaned[j]);
                inner.push(cleaned[j + 1]);
                j += 2;
                continue;
            }
            if cleaned[j] == b'"' {
                break;
            }
            inner.push(cleaned[j]);
            j += 1;
        }
        if let Ok(text) = String::from_utf8(inner) {
            if text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
                found.insert(text);
            }
        }
        i = j + 1; // resume after the closing quote
    }
    found
}

fn source_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            files.extend(source_files_under(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// 源码里出现的每个 catalog 键都必须在目录里真实存在。
///
/// 这个测试盯的是「动态拼键」造成的漏翻译：`cloud_sync.rs` 曾经用
/// `format!("cloudSyncE2ee.errors.{code}")` 拼键，`cloud_sync_ui.rs` 里还有一批
/// `states.* / step* / syncNow` 常量从未进过目录，结果界面上直接画出了
/// `cloudSyncE2ee.errors.service_unavailable`、`cloudSyncE2ee.states.disabled`。
///
/// 只看**字符串字面量**（不看注释与标识符），并跳过带 `{` 的格式模板：
/// 模板拼出来的键无法在编译期校验，只能靠 `error_key()` 那种字面量表。
#[test]
fn every_literal_catalog_key_exists_in_the_catalog() {
    fn string_literals(source: &str) -> Vec<String> {
        let cleaned = strip_comments(source);
        let mut out = Vec::new();
        let mut i = 0;
        while i < cleaned.len() {
            if cleaned[i] != b'"' {
                i += 1;
                continue;
            }
            let mut inner = Vec::new();
            let mut j = i + 1;
            while j < cleaned.len() {
                match cleaned[j] {
                    b'\\' if j + 1 < cleaned.len() => {
                        inner.push(cleaned[j + 1]);
                        j += 2;
                    }
                    b'"' => break,
                    byte => {
                        inner.push(byte);
                        j += 1;
                    }
                }
            }
            out.push(String::from_utf8_lossy(&inner).into_owned());
            i = j + 1;
        }
        out
    }

    let root = crate_root().join("src");
    let catalog = std::fs::read_to_string(root.join("i18n.rs")).unwrap();
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut rest = catalog.as_str();
    while let Some(index) = rest.find("key: \"") {
        let after = &rest[index + "key: \"".len()..];
        let Some(end) = after.find('"') else { break };
        known.insert(after[..end].to_string());
        rest = &after[end..];
    }
    assert!(!known.is_empty(), "the catalog must declare keys");

    const NAMESPACES: [&str; 2] = ["cloudSyncE2ee.", "settings."];
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for file in source_files_under(&root) {
        if file.strip_prefix(&root).unwrap().starts_with("i18n.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&file).unwrap();
        for literal in string_literals(&source) {
            let Some(namespace) = NAMESPACES.iter().find(|ns| literal.starts_with(**ns)) else {
                continue;
            };
            let key = literal.as_str();
            let looks_like_key = key.len() > namespace.len()
                && !key.ends_with('.')
                && !key.contains('{')
                && !key.contains('/')
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
            if looks_like_key && !known.contains(key) {
                missing.insert(key.to_string());
            }
        }
    }
    assert!(
        missing.is_empty(),
        "these catalog keys are referenced from src/ but missing from src/i18n.rs: {missing:?}"
    );
}

#[test]
fn shell_module_is_fully_localized_with_no_raw_simplified_chinese() {
    let shell = std::fs::read_to_string(crate_root().join("src/ui/shell.rs")).unwrap();
    let cleaned = String::from_utf8(strip_comments(&shell)).unwrap();
    let has_cjk = cleaned
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    assert!(
        !has_cjk,
        "src/ui/shell.rs must be fully localized (no raw Simplified-Chinese text)"
    );
}

#[test]
fn no_new_raw_simplified_chinese_user_visible_literals_outside_catalog_or_baseline() {
    let root = crate_root().join("src");
    // The translation catalog is the one sanctioned home for zh-CN text.
    let mut current: BTreeSet<String> = BTreeSet::new();
    for file in source_files_under(&root) {
        if file.strip_prefix(&root).unwrap().starts_with("i18n.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&file).unwrap();
        current.extend(cjk_string_literals(&source));
    }

    let baseline_path = crate_root().join("tests/zh_user_visible_baseline.txt");
    let baseline_raw = std::fs::read_to_string(&baseline_path).unwrap();
    let baseline: BTreeSet<String> = baseline_raw
        .lines()
        .map(str::to_string)
        .filter(|line| !line.trim().is_empty())
        .collect();

    let added: Vec<&String> = current.difference(&baseline).collect();
    assert!(
        added.is_empty(),
        "New raw Simplified-Chinese user-visible literal(s) were added to the Linux egui UI \
         outside the localization catalog. Translate them in src/i18n.rs and, if one is \
         deliberately dynamic/error text, refresh the baseline file. Found:\n{}",
        added
            .iter()
            .map(|s| format!("  - {s:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
