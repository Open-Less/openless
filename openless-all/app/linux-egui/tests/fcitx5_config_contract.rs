//! 输入法配置不变量契约（Tauri-free）。
//!
//! 宿主对输入法只做一件事：把热键注册给 fcitx5 插件。它**从不改动输入法自己的
//! 配置文件**。
//!
//! 这条不变量是踩过坑后补的：曾有一版认为「拼音把分号注册成快速短语触发键，会在
//! 插件之前吃掉 `Ctrl+Shift+;`」，于是启动时自动清空 `~/.config/fcitx5/conf/`
//! 里那一行。真机实测证伪了那个前提 —— fcitx 的 `Key::check` 要求修饰位精确相等
//! （`semicolon` 是 `states=0`，而 `Ctrl+Shift+;` 到达时是 `states=Ctrl`），分号
//! 占用与 QA 热键本来就能共存。那段逻辑留下的唯一效果是「每次启动偷偷改用户输入法
//! 配置」，因此删除。
//!
//! 需要在设置页**读取**（而不是写入）引擎配置时，请在这里显式放行并写明理由，
//! 不要悄悄绕过这条契约。

use std::path::{Path, PathBuf};

fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
            continue;
        }
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap()
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

/// 用户输入法配置目录：任何写入都必须落到这里，所以出现即违规。
const INPUT_METHOD_CONFIG_DIR: &str = ".config/fcitx5";
/// 引擎的按键保留设置：我们曾去清空它，绝不允许再次出现。
const ENGINE_RESERVED_KEY_SETTING: &str = "QuickPhraseKey";

#[test]
fn the_host_never_rewrites_the_input_method_configuration() {
    let root = crate_root();
    let mut violations = Vec::new();
    for file in source_files_under(&root.join("src")) {
        let source = strip_comments(&std::fs::read_to_string(&file).unwrap());
        for (index, line) in source.lines().enumerate() {
            if line.contains(INPUT_METHOD_CONFIG_DIR) || line.contains(ENGINE_RESERVED_KEY_SETTING)
            {
                violations.push(format!(
                    "{}:{}: {}",
                    file.strip_prefix(&root).unwrap().display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "The Linux egui host must never touch the input method's own configuration: it registers \
         hotkeys with the fcitx5 addon and leaves ~/.config/fcitx5 alone. Found:\n{}",
        violations.join("\n")
    );
}
