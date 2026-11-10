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
