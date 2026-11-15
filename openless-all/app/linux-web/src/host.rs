//! 宿主职责：进程级单实例。托盘与窗口显隐归渲染宿主（Qt / 浏览器）。

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
