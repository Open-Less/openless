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
