//! Windows TSF 文本服务通道（IME 提交/选区读取的宿主进程侧）。
//!
//! 与 `windows-ime/` 里的 C++ TSF DLL 通过 `WM_COPYDATA` 通信：
//! - [`protocol`]：线协议（tag、状态码、payload 编码、共享内存布局）；
//! - [`target`]：捕获前台焦点线程/窗口，决定消息发往哪个 TSF 实例；
//! - [`ipc`]：消息窗口发现、提交与查询轮询；
//! - [`session`]：一次插入会话的状态机（提交 → 轮询 → 终态）。
//!
//! 平台层只依赖 `HWND`/`pid`/`tid` 这类普通参数，不持有窗口框架对象。

#[cfg(target_os = "windows")]
pub mod ipc;
pub mod protocol;
#[cfg(target_os = "windows")]
pub mod session;
#[cfg(target_os = "windows")]
pub mod target;

#[cfg(target_os = "windows")]
pub use ipc::{ImeSubmitRequest, ImeSubmitTarget, WindowsImeIpcServer};
#[cfg(target_os = "windows")]
pub use session::{WindowsImeSessionController, WindowsImeSessionError};
