//! 宿主职责：单实例、托盘、窗口显隐。窗口渲染在 `main.rs`（GTK + WebKit）。
//!
//! 托盘直接复用 `openless-linux-egui` 里已调好的 StatusNotifierItem 实现。
//! 注意 `LinuxTray` 不是 `Send`（内部持有 mpsc Receiver），所以**整个托盘对象
//! 留在自己的线程里**，只把 `TrayCommand` 转发给 GTK 主线程。
use gtk::prelude::*;
use openless_linux_host::{LinuxTray, TrayCommand};

/// 单实例：占用 `$XDG_RUNTIME_DIR` 下的一个 socket。
///
/// 用 socket 而不是文件锁：进程崩掉时内核自动释放，不会留下"假占用"。
pub fn claim_single_instance() -> Option<std::os::unix::net::UnixListener> {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{dir}/openless-web.sock");
    let _ = std::fs::remove_file(&path);
    match std::os::unix::net::UnixListener::bind(&path) {
        Ok(listener) => {
            println!("[linux-web] 单实例锁已占用：{path}");
            Some(listener)
        }
        Err(error) => {
            eprintln!("[linux-web] 单实例锁占用失败（可能已有实例）：{error}");
            None
        }
    }
}

/// 起一个托盘，并把它的命令接到窗口上（窗口关闭 = 隐藏，进程常驻）。
pub fn start_tray(window: gtk::ApplicationWindow) {
    let (command_tx, command_rx) = std::sync::mpsc::channel::<TrayCommand>();
    std::thread::Builder::new()
        .name("openless-tray".into())
        .spawn(move || match LinuxTray::start() {
            Ok(tray) => {
                println!("[linux-web] 托盘已注册");
                loop {
                    tray.drain(|command| {
                        let _ = command_tx.send(command);
                    });
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
            Err(error) => eprintln!("[linux-web] 托盘注册失败：{error:?}"),
        })
        .ok();

    gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        while let Ok(command) = command_rx.try_recv() {
            match command {
                TrayCommand::ShowMain => window.present(),
                TrayCommand::Quit => {
                    if let Some(app) = window.application() {
                        app.quit();
                    }
                }
                TrayCommand::ActivatePreviousStyle | TrayCommand::SelectMicrophone(_) => {}
            }
        }
        gtk::glib::ControlFlow::Continue
    });
}
