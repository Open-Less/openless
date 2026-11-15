//! 进程级单实例。托盘与窗口显隐归渲染宿主（Qt / 浏览器）。

/// 单实例：占用 `$XDG_RUNTIME_DIR` 下的一个 socket。
///
/// 用 socket 而不是文件锁：进程崩掉时内核自动释放，不会留下"假占用"。
/// 但 `bind` 之前必须先判断旧 socket 后面还有没有活的进程 —— 直接 `unlink`
/// 再把同名 socket 绑上，等于把别人的锁抢掉，两个后端会同时写同一个数据目录。
pub fn claim_single_instance() -> Option<std::os::unix::net::UnixListener> {
    use std::os::unix::net::{UnixListener, UnixStream};

    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{dir}/openless-web.sock");
    if std::path::Path::new(&path).exists() {
        if UnixStream::connect(&path).is_ok() {
            eprintln!("[linux-web] 已有后端实例在运行（{path}），本次启动退出");
            return None;
        }
        // 连不上说明是上一次崩溃留下的空壳，可以安全回收。
        let _ = std::fs::remove_file(&path);
    }
    match UnixListener::bind(&path) {
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
