//! 全局热键：复用 fcitx5 插件的信号（与 egui 版同一套实现）。
//!
//! 两个线程约束：
//! 1. `Fcitx5HotkeyListener` 不是 `Send` —— 必须在它自己的线程里创建和使用；
//! 2. `ApplicationWindow` 也不是 `Send` —— 窗口操作只能回 GTK 主线程做。
//! 所以：线程里创建监听并把事件丢进 glib channel，主线程收事件再动窗口/起听写。
use gtk::glib;
use gtk::prelude::*;
use openless_core::OpenLessBackend;
use openless_linux_egui::{Fcitx5HotkeyListener, LinuxHotkeyEvent};
use std::sync::Arc;

/// 无 GTK 主循环时的热键监听（`OPENLESS_HEADLESS=1` 用）。
///
/// 与 [`spawn`] 的区别：不经过 glib channel，事件直接在自己的线程里处理，
/// 因为无头模式根本没有 GTK 事件循环（主线程只是 park）。
pub fn spawn_headless(
    backend: Arc<OpenLessBackend>,
    runtime: Arc<tokio::runtime::Runtime>,
) {
    std::thread::Builder::new()
        .name("openless-hotkeys".into())
        .spawn(move || match Fcitx5HotkeyListener::start() {
            Ok(listener) => {
                println!("[linux-web] 热键监听已启动（无头）");
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

pub fn spawn(
    backend: Arc<OpenLessBackend>,
    runtime: Arc<tokio::runtime::Runtime>,
    window: gtk::ApplicationWindow,
) {
    let (tx, rx) = glib::MainContext::channel::<LinuxHotkeyEvent>(glib::Priority::DEFAULT);

    std::thread::Builder::new()
        .name("openless-hotkeys".into())
        .spawn(move || match Fcitx5HotkeyListener::start() {
            Ok(listener) => {
                println!("[linux-web] 热键监听已启动（听写 / QA）");
                loop {
                    listener.drain(|event| {
                        let _ = tx.send(event);
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

    rx.attach(None, move |event| {
        match event {
            LinuxHotkeyEvent::DictationPressed { .. } => {
                let backend = Arc::clone(&backend);
                runtime.spawn(async move {
                    let session = backend.snapshot().dictation.session_id;
                    let result = match session {
                        Some(id) => backend.stop_dictation_session(id).await.map(|_| ()),
                        None => backend
                            .start_dictation_with_options(openless_core::DictationStartOptions::default())
                            .await
                            .map(|_| ()),
                    };
                    if let Err(error) = result {
                        eprintln!("[linux-web] 听写切换失败：{error}");
                    }
                });
            }
            LinuxHotkeyEvent::QaPressed => window.present(),
            _ => {}
        }
        glib::ControlFlow::Continue
    });
}
