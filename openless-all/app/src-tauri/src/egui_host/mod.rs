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
//!
//! ## 可替换的宿主边界（HostActions）
//! 本模块**不直接依赖** `Coordinator`。它只依赖一个 [`HostActions`] trait：
//! 主进程把子进程吐出的动作（确认/取消/提交/录音/关闭）翻译成语义化回调。
//! 现在这个 trait 由 Tauri 侧的 `CoordinatorAdapter` 实现（内部调 `Coordinator`）；
//! 将来替换掉 Tauri 时，只需换一个 `HostActions` 实现，本模块与 `egui_popup`
//! 进程、以及稳定的 JSON 协议都不动。这就是「切换期留出的空间」。

pub mod fonts;
pub mod qa;
pub mod qa_event;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use crate::coordinator::Coordinator;
use qa_event::QaStateEvent;
use tauri::Manager;

/// 是否启用 egui 浮窗宿主：Linux + 未显式禁用。
/// 环境变量 `OPENLESS_EGUI_PREVIEW=0` 可关闭（用于回退 WebView / 排查）。
pub fn enabled() -> bool {
    cfg!(target_os = "linux") && std::env::var("OPENLESS_EGUI_PREVIEW").as_deref() != Ok("0")
}

// ───────────────────────── 宿主动作边界（薄适配层）─────────────────────────

/// 弹窗子进程吐出的、需要宿主响应的语义化动作。
/// 这是与 Tauri 解耦的边界：将来替换宿主只需实现这个 trait。
#[allow(dead_code)]
pub trait HostActions: Send + Sync + 'static {
    /// 用户确认了划词润色预览（可能编辑过 `text`）。
    fn confirm_selection_polish_preview(&self, text: String) -> Result<(), String>;
    /// 用户取消了划词润色预览。
    fn cancel_selection_polish_preview(&self);
    /// 用户关闭了 QA 面板。
    fn qa_window_dismiss(&self);
    /// 用户提交了一个问题（异步）。
    fn qa_submit_text(&self, text: String);
    /// 用户切换了录音状态（异步）。
    fn qa_toggle_recording(&self);
}

/// 读取 QA 弹窗当前 selection 预览 payload（同步，供 show_preview 取初始内容）。
#[allow(dead_code)]
pub trait HostPoller {
    fn selection_polish_preview(&self) -> Option<SelectionPreviewPayload>;
}

/// 划词润色预览的初始 payload（与 coordinator 侧结构对齐的轻量定义）。
#[derive(Clone)]
#[allow(dead_code)]
pub struct SelectionPreviewPayload {
    pub text: String,
    pub source_text: String,
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
    let Some(host) = app.try_state::<Arc<dyn EguiHost>>().map(|s| s.inner().clone()) else {
        log::warn!("[egui-host] egui host state unavailable");
        return false;
    };
    let Some(payload) = host.selection_polish_preview() else {
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

    // 后台线程读 stdout：收到 confirm/cancel 就回调宿主动作。
    let stdout = child.stdout.take().expect("stdout piped");
    let host_for_thread = host.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.contains("\"confirm\"") {
                let text = extract_str(trimmed, "text").unwrap_or_default();
                let _ = host_for_thread.confirm_selection_polish_preview(text);
                break;
            } else if trimmed.contains("\"cancel\"") {
                host_for_thread.cancel_selection_polish_preview();
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
    enabled()
}

// ───────────────────────── QA 面板（常驻进程）─────────────────────────

/// QA 子进程生命周期（进程级单例）。
struct QaProc {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
}

impl QaProc {
    fn new() -> Self {
        Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
        }
    }
}

/// 用 `OnceLock` 存 QA 进程单例。
static QA_PROC: OnceLock<QaProc> = OnceLock::new();
fn qa_proc() -> &'static QaProc {
    QA_PROC.get_or_init(QaProc::new)
}

/// 打开 QA 问答面板窗。成功返回 true；未启用/无法启动返回 false，回退 WebView。
pub(crate) fn show_qa<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    if !enabled() {
        return false;
    }
    let Some(host) = app.try_state::<Arc<dyn EguiHost>>().map(|s| s.inner().clone()) else {
        log::warn!("[egui-host] egui host state unavailable");
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

    // 后台线程读 stdout：把 QA 用户操作分发给 HostActions。
    let stdout = {
        let mut guard = proc.child.lock().unwrap();
        guard.as_mut().and_then(|c| c.stdout.take()).expect("stdout piped")
    };
    let host_for_thread = host.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.contains("\"submit\"") {
                let text = extract_str(trimmed, "text").unwrap_or_default();
                host_for_thread.qa_submit_text(text);
            } else if trimmed.contains("\"toggle_record\"") {
                host_for_thread.qa_toggle_recording();
            } else if trimmed.contains("\"dismiss\"") {
                host_for_thread.qa_window_dismiss();
                break;
            }
        }
        // stdout 关闭（子进程退出）→ 清空 child（回收，避免僵尸进程）。
        if let Some(mut c) = qa_proc().child.lock().unwrap().take() {
            let _ = c.wait();
        }
        *qa_proc().stdin.lock().unwrap() = None;
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
    }
    if let Some(mut child) = proc.child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
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

// ───────────────────────── Coordinator 适配层（临时接线）─────────────────────────

/// 宿主要实现的动作 + 数据拉取。组合 trait，方便用单个 trait 对象走 state。
#[allow(dead_code)]
pub trait EguiHost: HostActions + HostPoller {}

/// 把 [`HostActions`] 桥接到 Tauri 侧的 [`Coordinator`] 实现。
/// 将来替换 Tauri 时，仅替换此适配层，`egui_host` 与 `egui_popup` 均不动。
#[allow(dead_code)]
pub struct CoordinatorHostAdapter {
    coordinator: Arc<Coordinator>,
}

impl CoordinatorHostAdapter {
    pub fn new(coordinator: Arc<Coordinator>) -> Self {
        Self { coordinator }
    }
}

impl EguiHost for CoordinatorHostAdapter {}

impl HostPoller for CoordinatorHostAdapter {
    fn selection_polish_preview(&self) -> Option<SelectionPreviewPayload> {
        self.coordinator.selection_polish_preview().map(|p| SelectionPreviewPayload {
            text: p.text,
            source_text: p.source_text,
        })
    }
}

impl HostActions for CoordinatorHostAdapter {
    fn confirm_selection_polish_preview(&self, text: String) -> Result<(), String> {
        self.coordinator.confirm_selection_polish_preview(text)
    }
    fn cancel_selection_polish_preview(&self) {
        self.coordinator.cancel_selection_polish_preview();
    }
    fn qa_window_dismiss(&self) {
        self.coordinator.qa_window_dismiss();
    }
    fn qa_submit_text(&self, text: String) {
        let coord = self.coordinator.clone();
        tauri::async_runtime::spawn(async move {
            let _ = coord.qa_submit_text(text).await;
        });
    }
    fn qa_toggle_recording(&self) {
        let coord = self.coordinator.clone();
        tauri::async_runtime::spawn(async move {
            coord.qa_toggle_recording().await;
        });
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
