//! 全局热键：复用 fcitx5 插件的信号（与 Linux 宿主是同一套实现）。
//!
//! 约束：`Fcitx5HotkeyListener` 不是 `Send` —— 必须在它自己的线程里创建和使用，
//! 所以监听常驻一个专用线程，事件在该线程里直接派发。
use openless_core::OpenLessBackend;
use openless_linux_host::{Fcitx5HotkeyListener, LinuxHotkeyEvent};
use std::sync::Arc;

/// 起热键监听：快捷键 → 听写 / 语音提问。
pub fn spawn_headless(
    backend: Arc<OpenLessBackend>,
    runtime: Arc<tokio::runtime::Runtime>,
) {
    std::thread::Builder::new()
        .name("openless-hotkeys".into())
        .spawn(move || match Fcitx5HotkeyListener::start() {
            Ok(listener) => {
                println!("[linux-web] 热键监听已启动（听写 / QA）");
                loop {
                    listener.drain(|event| match event {
                        LinuxHotkeyEvent::DictationPressed { .. } => {
                            let backend = Arc::clone(&backend);
                            runtime.spawn(async move {
                                let session = backend.snapshot().dictation.session_id;
                                let result = match session {
                                    Some(id) => {
                                        backend.stop_dictation_session(id).await.map(|_| ())
                                    }
                                    None => backend
                                        .start_dictation_with_options(
                                            openless_core::DictationStartOptions::default(),
                                        )
                                        .await
                                        .map(|_| ()),
                                };
                                if let Err(error) = result {
                                    eprintln!("[linux-web] 听写切换失败：{error}");
                                }
                            });
                        }
                        LinuxHotkeyEvent::SelectionPolishPressed => {
                            // 选区助手：读当前选区（fcitx5 surrounding/selection）→
                            // 润色 → 用 SelectionRuntimeAdapter 替换回原处。
                            // previewConfirm 需要预览确认 UI，Linux 还没有，先按直替处理。
                            let backend = Arc::clone(&backend);
                            runtime.spawn(async move {
                                let mode = backend.get_preferences().selection_polish_output_mode;
                                if mode != openless_core::SelectionPolishOutputMode::DirectReplace
                                {
                                    log::warn!(
                                        "[selection] previewConfirm is not available on Linux yet; \
                                         applying the polished text directly"
                                    );
                                }
                                match backend
                                    .services()
                                    .selection
                                    .begin_polish(openless_core::SelectionPolishRequest {
                                        selected_text: None,
                                        mode: openless_core::PolishMode::Raw,
                                        instruction: None,
                                    })
                                    .await
                                {
                                    Ok(session) => {
                                        log::info!("[selection] polished selection session {session}")
                                    }
                                    Err(error) => {
                                        log::warn!("[selection] polish failed: {error}");
                                    }
                                }
                            });
                        }
                        LinuxHotkeyEvent::QaPressed => {
                            // 问答热键：开/关一次语音提问，状态通过 qa:state 推给宿主。
                            let backend = Arc::clone(&backend);
                            runtime.spawn(async move {
                                if let Err(error) =
                                    backend.services().qa.toggle_recording().await
                                {
                                    eprintln!("[linux-web] QA 切换失败：{error}");
                                }
                            });
                        }
                        _ => {}
                    });
                    if listener.take_error().is_some() {
                        eprintln!("[linux-web] fcitx5 热键监听断开");
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            Err(error) => eprintln!("[linux-web] fcitx5 热键监听启动失败：{error}"),
        })
        .ok();
}
