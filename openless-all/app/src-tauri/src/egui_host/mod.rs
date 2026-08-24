//! # egui 浮窗宿主（egui host）
//!
//! 在 **Linux** 上用 egui 原生窗口渲染弹窗，替代 Tauri WebView 浮窗。
//! 采用「**每个弹窗一个独立进程、单窗口**」的方案，避免 eframe 多 viewport 在
//! Wayland 下的局限性（root 泄漏 / child 内容闪烁）。每个弹窗的 UI 是仓库里的
//! 独立二进制 `egui_popup`（`src/bin/egui_popup.rs`），主进程按需 spawn、用
//! stdin/stdout JSON 行通信、不用时 kill。
//!
//! 当前覆盖两枚弹窗：
//! - **选区润色预览**（`--preview`）—— 一次性进程。
//! - **QA 问答面板**（`--qa`）—— 常驻进程，流式双向 IPC。
//!
//! 其它平台（macOS/Windows/Android/iOS）维持 WebView 版，本模块整体
//! `#[cfg(target_os = "linux")]` 门控。
//!
//! ## 为何用独立进程而非多 viewport
//! eframe 0.36 的 `show_viewport_deferred` + 隐藏 root 在 Wayland 下实测 root 会
//! 泄漏成可见窗、child 内容渲染不稳定（一闪而过）。独立进程单窗口是 Wayland/X11
//! 下最简单、最稳妥的形态：每个进程只有一个原生窗口，没有隐藏宿主、没有多
//! viewport 生命周期，不用时杀掉进程即可。

pub mod fonts;
pub(crate) mod qa;
pub(crate) mod qa_event;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::coordinator::Coordinator;
use qa_event::QaStateEvent;
use tauri::Manager;

/// 是否启用 egui 浮窗宿主：Linux + 未显式禁用。
/// 环境变量 `OPENLESS_EGUI_PREVIEW=0` 可关闭（用于回退 WebView / 排查）。
pub(crate) fn enabled() -> bool {
    cfg!(target_os = "linux") && std::env::var("OPENLESS_EGUI_PREVIEW").as_deref() != Ok("0")
}

// ───────────────────────── 独立进程可执行路径 ─────────────────────────

/// 定位 `egui_popup` 可执行文件路径。
/// 优先用当前 exe 所在目录（开发/发布 sibling），否则回退到 PATH。
fn popup_bin_path() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("egui_popup");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    // 回退：target/debug 或 target/release（开发便捷）。
    if let Ok(cwd) = std::env::current_dir() {
        for sub in ["target/debug/egui_popup", "target/release/egui_popup"] {
            let candidate = cwd.join(sub);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

// ───────────────────────── 预览窗（一次性进程）─────────────────────────

/// 打开划词润色预览窗（专用入口）。成功返回 true；未启用/无 pending payload /
/// 无法定位子进程，返回 false，由 lib.rs 回退到 WebView 版。
pub(crate) fn show_preview<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    if !enabled() {
        return false;
    }
    let Some(coordinator) = app.try_state::<Arc<Coordinator>>().map(|s| s.inner().clone()) else {
        log::warn!("[egui-host] coordinator state unavailable");
        return false;
    };
    let Some(payload) = coordinator.selection_polish_preview() else {
        log::warn!("[egui-host] no pending preview payload");
        return false;
    };
    let Some(bin) = popup_bin_path() else {
        log::warn!("[egui-host] egui_popup binary not found; falling back to WebView");
        return false;
    };

    let mut child = match Command::new(&bin)
        .arg("--preview")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(error) => {
            log::warn!("[egui-host] spawn egui_popup failed: {error}");
            return false;
        }
    };

    // 写入初始 payload。
    if let Some(stdin) = child.stdin.as_mut() {
        let line = format!(
            "{{\"action\":\"set\",\"text\":{},\"source\":{}}}\n",
            json_escape(&payload.text),
            json_escape(&payload.source_text)
        );
        let _ = stdin.write_all(line.as_bytes());
        let _ = stdin.flush();
    }

    // 后台线程读 stdout：收到 confirm/cancel 就回调 coordinator。
    let stdout = child.stdout.take().expect("stdout piped");
    let coordinator_for_thread = coordinator.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.contains("\"confirm\"") {
                let text = extract_str(trimmed, "text").unwrap_or_default();
                let _ = coordinator_for_thread.confirm_selection_polish_preview(text);
                break;
            } else if trimmed.contains("\"cancel\"") {
                coordinator_for_thread.cancel_selection_polish_preview();
                break;
            }
        }
    });

    // 等待进程结束，回收 child（避免僵尸进程）。
    let _ = child.wait();
    true
}

pub(crate) fn hide_preview() -> bool {
    // 预览窗是一次性进程，用户确认/取消后会自行退出；hide 无需额外动作。
    // 返回 true 表示「已由 egui 接管」（未启用时返回 false 让 WebView 兜底）。
    enabled()
}

// ───────────────────────── QA 面板（常驻进程）─────────────────────────

/// QA 子进程管理器（进程级单例）。
struct QaProcess {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
}

static QA_PROC: OnceLock<QaProcess> = OnceLock::new();

fn qa_proc() -> &'static QaProcess {
    QA_PROC.get_or_init(|| QaProcess {
        child: Mutex::new(None),
        stdin: Mutex::new(None),
    })
}

/// 打开 QA 问答面板窗。成功返回 true；未启用/无法启动返回 false，回退 WebView。
pub(crate) fn show_qa<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    if !enabled() {
        return false;
    }
    let Some(coordinator) = app.try_state::<Arc<Coordinator>>().map(|s| s.inner().clone()) else {
        log::warn!("[egui-host] coordinator state unavailable");
        return false;
    };
    let Some(bin) = popup_bin_path() else {
        log::warn!("[egui-host] egui_popup binary not found; falling back to WebView");
        return false;
    };

    let proc = qa_proc();
    // 已有进程在跑则复用（重复打开只需窗口已存在）。
    if proc.child.lock().unwrap().is_some() {
        return true;
    }

    let mut child = match Command::new(&bin)
        .arg("--qa")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(error) => {
            log::warn!("[egui-host] spawn egui_popup --qa failed: {error}");
            return false;
        }
    };

    let stdin = child.stdin.take();
    *proc.stdin.lock().unwrap() = stdin;
    *proc.child.lock().unwrap() = Some(child);

    // 后台线程读 stdout：处理 QA 用户操作 → coordinator。
    let mut child_to_wait = proc.child.lock().unwrap().take();
    let stdout = child_to_wait
        .as_mut()
        .and_then(|c| c.stdout.take())
        .expect("stdout piped");
    let coordinator_for_thread = coordinator.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.contains("\"submit\"") {
                let text = extract_str(trimmed, "text").unwrap_or_default();
                let coord = coordinator_for_thread.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = coord.qa_submit_text(text).await;
                });
            } else if trimmed.contains("\"toggle_record\"") {
                let coord = coordinator_for_thread.clone();
                tauri::async_runtime::spawn(async move {
                    coord.qa_toggle_recording().await;
                });
            } else if trimmed.contains("\"dismiss\"") {
                coordinator_for_thread.qa_window_dismiss();
            }
        }
        // stdout 关闭（子进程退出）→ 清空 child。
        *qa_proc().child.lock().unwrap() = None;
        *qa_proc().stdin.lock().unwrap() = None;
        // 回收子进程。
        if let Some(mut c) = child_to_wait {
            let _ = c.wait();
        }
    });

    log::info!("[egui-host] qa egui_popup spawned");
    true
}

/// 隐藏 QA 面板：向子进程发 hide（子进程自行退出），并回收。
pub(crate) fn hide_qa() -> bool {
    if !enabled() {
        return false;
    }
    let proc = qa_proc();
    if let Some(mut stdin) = proc.stdin.lock().unwrap().take() {
        let _ = stdin.write_all(b"{\"action\":\"hide\"}\n");
        let _ = stdin.flush();
        // 等待子进程退出。
        if let Some(mut child) = proc.child.lock().unwrap().take() {
            let _ = child.kill(); // 兜底：若 hide 未让进程退出则强制结束。
            let _ = child.wait();
        }
    }
    true
}

/// 把后端一个 `qa:state` 事件推入 QA 子进程（由 lib.rs 的 `app.listen_any` 调用）。
pub(crate) fn push_qa_state(event: QaStateEvent) {
    if !enabled() {
        return;
    }
    let proc = qa_proc();
    let payload = serde_json::to_string(&event).unwrap_or_default();
    let line = format!("{{\"action\":\"qa:state\",\"payload\":{payload}}}\n");
    if let Some(mut stdin) = proc.stdin.lock().unwrap().as_mut() {
        let _ = stdin.write_all(line.as_bytes());
        let _ = stdin.flush();
    }
}

// ───────────────────────── JSON 辅助（与 bin 一致）─────────────────────────

fn extract_str(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let mut rest = line[start..].trim_start();
    if rest.starts_with('{') {
        let mut depth = 0;
        let mut in_str = false;
        let mut out = String::new();
        for ch in rest.chars() {
            if in_str {
                out.push(ch);
                if ch == '"' {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '\\' => {
                    out.push(ch);
                    in_str = true;
                }
                '"' => {
                    out.push(ch);
                    in_str = false;
                }
                '{' => {
                    depth += 1;
                    out.push(ch);
                }
                '}' => {
                    depth -= 1;
                    out.push(ch);
                    if depth == 0 {
                        break;
                    }
                }
                _ => out.push(ch),
            }
        }
        return Some(out);
    }
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        let raw = &stripped[..end];
        Some(raw.replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\"))
    } else {
        None
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}
