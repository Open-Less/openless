//! 宿主进程 ⇄ UI 窗口进程之间的 IPC。
//!
//! 形态：**常驻无窗口宿主**（后端 / 数据 / 单实例锁 / 热键 / 托盘 / 弹窗）与
//! **可随时开关的 UI 窗口进程**（纯渲染，绝不碰后端与数据目录）。关窗只是窗口
//! 进程退出（任务栏条目随之消失），宿主与后端状态原地不动。
//!
//! 时序契约（两端都必须守）：
//! 1. 宿主先抢到单实例锁、起好托盘与热键，**然后**才 bind 本 socket；
//! 2. socket 就绪后才拉 UI 进程 —— UI 必须连上宿主才渲染任何数据；
//! 3. 快照带单调递增 `sequence`，UI 只接受更大的序号，旧包直接丢，绝不回退；
//! 4. UI 断连（EOF）后宿主立即作废窗口句柄，但会话/录音/弹窗不受影响；
//! 5. 宿主退出时先给 UI 发 `Shutdown`，再释放单实例锁。
//!
//! 帧格式与弹窗协议一致：一行一个 JSON 对象，便于用既有工具排查。

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::frontend::view_model::{FrontendAction, FrontendViewModel};

/// 协议版本：宿主与 UI 进程对不上就直接拒绝启动 UI（避免半懂不懂地渲染）。
pub const UI_BRIDGE_VERSION: u32 = 2;

/// 单帧上限。视图模型快照含历史列表，比弹窗协议大得多。
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// UI 进程等待宿主 socket 出现的上限（宿主 bind 后才拉它，正常是毫秒级）。
pub const UI_CLIENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostToWindow {
    /// 握手确认：UI 收到它才算真的接上，此前不渲染任何业务数据。
    Ready {
        version: u32,
    },
    Rejected {
        reason: String,
    },
    FocusMain,
    /// 本地热键匹配所需的配置。
    ///
    /// UI 窗口进程有焦点时 fcitx5 收不到按键（见 `crate::local_hotkeys` 的模块
    /// 文档），所以窗口自己按这份配置匹配。宿主在窗口连上后、以及配置变化后
    /// 各发一次；与视图模型快照分开，避免让含历史列表的大快照为几个绑定加宽。
    Hotkeys {
        version: u32,
        bindings: Box<openless_core::HotkeyRuntimeTarget>,
    },
    /// 完整视图模型快照；`sequence` 单调递增。
    Snapshot {
        sequence: u64,
        view_model: Box<FrontendViewModel>,
    },
    /// 延迟探针回包。
    Pong {
        sequence: u64,
    },
    /// 宿主退出，UI 自行关窗退出。
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowToHost {
    /// UI 已连上，报告自己的协议版本。
    Hello { version: u32 },
    /// 用户动作，按发送顺序处理。
    Action {
        sequence: u64,
        action: Box<FrontendAction>,
    },
    /// 延迟探针。
    Ping { sequence: u64 },
    /// 本窗口内命中的热键。
    ///
    /// 插件只在其聚焦的客户端注册了 text-input 时才收得到按键，我们的窗口从不
    /// 注册 —— 所以窗口有焦点时热键只能由窗口自己认出来，作为边沿送回宿主，
    /// 由宿主合成与插件信号等价的 `LinuxHotkeyEvent`（并按热键身份去重）。
    Hotkey {
        sequence: u64,
        edge: openless_linux_egui::LocalHotkeyEdge,
    },
    /// UI 正常退出前的告别（宿主据此立即作废句柄，不必等 EOF）。
    Bye,
}

/// 宿主与 UI 进程约定的 socket 路径（放在 XDG_RUNTIME_DIR 下）。
pub fn ui_socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("openless-ui.sock")
}

/// 写一帧 JSONL。
pub fn write_frame<T: Serialize>(writer: &mut impl Write, frame: &T) -> std::io::Result<()> {
    let mut encoded = serde_json::to_vec(frame)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("UI bridge frame is {} bytes", encoded.len()),
        ));
    }
    encoded.push(b'\n');
    writer.write_all(&encoded)?;
    writer.flush()
}

/// 读一帧 JSONL；EOF 时返回 `Ok(None)`，便于把「对方退出」与「帧损坏」分开。
pub fn read_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut impl BufRead,
) -> std::io::Result<Option<T>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "incomplete UI frame",
            ));
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(available.len(), |index| index + 1);
        if line.len() + count > MAX_FRAME_BYTES + 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "UI bridge frame too large",
            ));
        }
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if end.is_some() {
            break;
        }
    }
    let frame = serde_json::from_slice(&line[..line.len() - 1])
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok(Some(frame))
}

/// 一个已连上的 UI 窗口。读线程负责 `incoming`，写线程负责 `outgoing`。
struct UiConnection {
    ready: bool,
    deadline: std::time::Instant,
    action_sequence: u64,
    hotkey_sequence: u64,
    incoming: Receiver<WindowToHost>,
    outgoing: Sender<Outgoing>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
    /// socket 的一份副本：收尾时 `shutdown(Both)` 才能把阻塞在 read 上的
    /// 读线程叫醒，否则 join 会一直等下去。
    control: UnixStream,
}

enum Outgoing {
    /// 快照会合并：排在后面的覆盖前面还没写出去的，避免 UI 卡顿时堆一堆过期状态。
    Frame(HostToWindow),
    /// UI 进程发往宿主的帧（动作、探针、告别）。
    ClientFrame(WindowToHost),
    /// 已编码的快照帧（宿主为了算指纹已经序列化过，避免重复序列化）。
    Encoded(Vec<u8>),
    Stop,
}

impl Outgoing {
    fn is_snapshot(&self) -> bool {
        matches!(
            self,
            Outgoing::Frame(HostToWindow::Snapshot { .. }) | Outgoing::Encoded(_)
        )
    }

    fn write_to(&self, stream: &mut UnixStream) -> std::io::Result<()> {
        match self {
            Outgoing::Stop => Ok(()),
            Outgoing::Frame(frame) => write_frame(stream, frame),
            Outgoing::ClientFrame(frame) => write_frame(stream, frame),
            Outgoing::Encoded(bytes) => {
                if bytes.len() > MAX_FRAME_BYTES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "UI bridge frame too large",
                    ));
                }
                stream.write_all(bytes)?;
                stream.write_all(b"\n")?;
                stream.flush()
            }
        }
    }
}

/// 宿主侧桥：监听 ≤1 个 UI 窗口连接，并暴露「发快照 / 收动作」的最小接口。
pub struct UiBridgeHost {
    listener: UnixListener,
    path: PathBuf,
    connection: Option<UiConnection>,
    /// 下一个要发出的快照序号（单调递增；UI 侧拒收更小的序号）。
    next_sequence: u64,
    /// 上一个 UI 连接的整体标识，用于日志。
    connection_generation: u64,
}

impl UiBridgeHost {
    /// bind 监听 socket。必须在抢到单实例锁、起好托盘/热键之后、拉 UI 进程之前调用。
    pub fn bind(path: PathBuf) -> std::io::Result<Self> {
        // 上一次异常退出可能留下 socket 文件；监听前先清掉，否则 bind 会 EADDRINUSE。
        match std::fs::remove_file(&path) {
            Ok(()) => log::info!("[ui-host] removed stale UI bridge socket"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!("[ui-host] stale socket not removable: {error}"),
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path,
            connection: None,
            next_sequence: 1,
            connection_generation: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 收下新连接（若有）。同一时刻只服务一个窗口；重复连接时保留先到的那个。
    pub fn accept_pending(&mut self) {
        if self.connection.is_some() {
            return;
        }
        match self.listener.accept() {
            Ok((stream, _)) => {
                self.connection_generation += 1;
                let generation = self.connection_generation;
                log::info!("[ui-host] UI window connected (generation {generation})");
                match spawn_connection(stream) {
                    Ok(connection) => self.connection = Some(connection),
                    Err(error) => {
                        log::warn!("[ui-host] UI window connection setup failed: {error}")
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => log::warn!("[ui-host] accept failed: {error}"),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connection
            .as_ref()
            .is_some_and(|connection| connection.ready)
    }

    /// 取走 UI 发来的消息（非阻塞，保持到达顺序）。
    pub fn drain(&mut self) -> Vec<WindowToHost> {
        let mut messages = Vec::new();
        let mut disconnected = false;
        let mut rejection = None;
        if let Some(connection) = self.connection.as_mut() {
            if !connection.ready && std::time::Instant::now() >= connection.deadline {
                rejection =
                    Some("UI handshake timed out; restart the complete application".to_string());
            } else {
                loop {
                    match connection.incoming.try_recv() {
                        Ok(WindowToHost::Hello { version }) if !connection.ready => {
                            if version != UI_BRIDGE_VERSION {
                                rejection = Some(format!("UI protocol {version} does not match {UI_BRIDGE_VERSION}; restart the complete application"));
                                break;
                            }
                            connection.ready = true;
                            let _ =
                                connection
                                    .outgoing
                                    .send(Outgoing::Frame(HostToWindow::Ready {
                                        version: UI_BRIDGE_VERSION,
                                    }));
                            messages.push(WindowToHost::Hello { version });
                        }
                        Ok(message) if connection.ready => {
                            let accepted = match &message {
                                WindowToHost::Action { sequence, .. } => {
                                    advance_sequence(&mut connection.action_sequence, *sequence)
                                }
                                WindowToHost::Hotkey { sequence, .. } => {
                                    advance_sequence(&mut connection.hotkey_sequence, *sequence)
                                }
                                WindowToHost::Hello { .. } => {
                                    rejection = Some("duplicate UI handshake".into());
                                    break;
                                }
                                _ => true,
                            };
                            if accepted {
                                messages.push(message);
                            }
                        }
                        Ok(_) => {
                            rejection = Some("Hello is required before UI actions".into());
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
            }
        }
        if let Some(reason) = rejection {
            if let Some(mut connection) = self.connection.take() {
                let _ = connection
                    .outgoing
                    .send(Outgoing::Frame(HostToWindow::Rejected { reason }));
                let _ = connection.outgoing.send(Outgoing::Stop);
                if let Some(writer) = connection.writer.take() {
                    let _ = writer.join();
                }
            }
            messages.clear();
        } else if disconnected {
            self.connection = None;
        }
        messages
    }

    /// 发送快照，`payload` 必须是 `view_model` 的 JSON 编码。
    ///
    /// 宿主为了判断「视图模型变没变」已经序列化过一次，这里直接拼帧、不再二次
    /// 序列化；`sequence` 由桥推进，保证单调递增。
    pub fn send_snapshot_encoded(&mut self, payload: &[u8]) {
        if !self.is_connected() {
            return;
        }
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let mut frame = Vec::with_capacity(payload.len() + 64);
        frame.extend_from_slice(b"{\"Snapshot\":{\"sequence\":");
        frame.extend_from_slice(sequence.to_string().as_bytes());
        frame.extend_from_slice(b",\"view_model\":");
        frame.extend_from_slice(payload);
        frame.extend_from_slice(b"}}");
        if frame.len() > MAX_FRAME_BYTES {
            log::error!("[ui-host] snapshot exceeds the UI frame limit");
            self.connection = None;
            return;
        }
        if let Some(connection) = self.connection.as_ref() {
            if connection.outgoing.send(Outgoing::Encoded(frame)).is_err() {
                self.connection = None;
            }
        }
    }

    pub fn send(&mut self, frame: HostToWindow) {
        if !self.is_connected() {
            return;
        }
        if let Some(connection) = self.connection.as_ref() {
            if connection.outgoing.send(Outgoing::Frame(frame)).is_err() {
                self.connection = None;
            }
        }
    }

    /// 宿主退出前的收尾：通知 UI 关窗，然后断开。
    pub fn shutdown(&mut self) {
        if let Some(mut connection) = self.connection.take() {
            let _ = connection
                .outgoing
                .send(Outgoing::Frame(HostToWindow::Shutdown));
            if let Some(writer) = connection.writer.take() {
                let _ = writer.join();
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn advance_sequence(last: &mut u64, sequence: u64) -> bool {
    if sequence <= *last {
        return false;
    }
    *last = sequence;
    true
}

impl Drop for UiConnection {
    fn drop(&mut self) {
        let _ = self.outgoing.send(Outgoing::Stop);
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

impl Drop for UiBridgeHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn write_outgoing(stream: &mut UnixStream, outgoing_rx: Receiver<Outgoing>) {
    // 待发快照只保留最新一份：UI 慢的时候宁可跳帧，也不能画过期状态。
    let mut pending: Option<Outgoing> = None;
    loop {
        let received = outgoing_rx.recv_timeout(Duration::from_millis(20));
        match received {
            Ok(Outgoing::Stop) => return,
            Ok(outgoing) if outgoing.is_snapshot() => pending = Some(outgoing),
            Ok(outgoing) => {
                let terminal = matches!(
                    outgoing,
                    Outgoing::Frame(HostToWindow::Shutdown | HostToWindow::Rejected { .. })
                );
                // Control frames precede the unsent snapshot. Shutdown
                // discards it, so stale state cannot follow a terminal frame.
                if outgoing.write_to(stream).is_err() || terminal {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(stale) = pending.take() {
                    if stale.write_to(stream).is_err() {
                        return;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn spawn_connection(stream: UnixStream) -> std::io::Result<UiConnection> {
    stream.set_write_timeout(Some(Duration::from_millis(250)))?;
    let reader_stream = stream.try_clone()?;
    let control = stream.try_clone()?;
    let (incoming_tx, incoming_rx) = mpsc::channel();
    let (outgoing_tx, outgoing_rx) = mpsc::channel::<Outgoing>();
    let reader_outgoing = outgoing_tx.clone();

    let reader = std::thread::Builder::new()
        .name("openless-ui-host-reader".into())
        .spawn(move || {
            let mut reader = BufReader::new(reader_stream);
            loop {
                match read_frame::<WindowToHost>(&mut reader) {
                    Ok(Some(frame)) => {
                        if incoming_tx.send(frame).is_err() {
                            break;
                        }
                    }
                    // EOF：UI 进程退出（正常关闭或被杀）。
                    Ok(None) => break,
                    Err(error) => {
                        log::warn!("[ui-host] UI bridge frame error: {error}");
                        break;
                    }
                }
            }
            let _ = reader_outgoing.send(Outgoing::Stop);
            let _ = reader.get_ref().shutdown(std::net::Shutdown::Both);
        })?;

    let writer = std::thread::Builder::new()
        .name("openless-ui-host-writer".into())
        .spawn(move || {
            let mut stream = stream;
            write_outgoing(&mut stream, outgoing_rx);
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });
    let writer = match writer {
        Ok(writer) => writer,
        Err(error) => {
            let _ = control.shutdown(std::net::Shutdown::Both);
            let _ = reader.join();
            return Err(error);
        }
    };

    Ok(UiConnection {
        ready: false,
        deadline: std::time::Instant::now() + UI_CLIENT_CONNECT_TIMEOUT,
        action_sequence: 0,
        hotkey_sequence: 0,
        incoming: incoming_rx,
        outgoing: outgoing_tx,
        reader: Some(reader),
        writer: Some(writer),
        control,
    })
}

/// UI 进程侧连接：连上宿主、收快照、发动作。
pub struct UiBridgeClient {
    incoming: Receiver<HostToWindow>,
    outgoing: Sender<Outgoing>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
    /// 见 `UiConnection::control`：收尾要用它叫醒读线程。
    control: UnixStream,
    ready: bool,
}

impl UiBridgeClient {
    /// 连宿主的 socket。宿主 bind 之后才拉 UI，所以这里通常一次就成；
    /// 仍做重试以覆盖「宿主刚 bind 就被调度器换出」的竞态。
    pub fn connect(path: &Path) -> Result<Self, String> {
        let deadline = std::time::Instant::now() + UI_CLIENT_CONNECT_TIMEOUT;
        let mut stream = loop {
            match UnixStream::connect(path) {
                Ok(stream) => break stream,
                Err(error) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(format!("UI bridge connect failed: {error}"));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };
        stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(UI_CLIENT_CONNECT_TIMEOUT))
            .map_err(|error| error.to_string())?;
        write_frame(
            &mut stream,
            &WindowToHost::Hello {
                version: UI_BRIDGE_VERSION,
            },
        )
        .map_err(|error| error.to_string())?;
        let reader_stream = stream
            .try_clone()
            .map_err(|error| format!("UI bridge clone failed: {error}"))?;
        let control = stream
            .try_clone()
            .map_err(|error| format!("UI bridge clone failed: {error}"))?;
        let (incoming_tx, incoming_rx) = mpsc::channel();
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<Outgoing>();
        let reader_outgoing = outgoing_tx.clone();
        let reader = std::thread::Builder::new()
            .name("openless-ui-client-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(reader_stream);
                match read_frame::<HostToWindow>(&mut reader) {
                    Ok(Some(HostToWindow::Ready { version })) if version == UI_BRIDGE_VERSION => {
                        let _ = reader.get_ref().set_read_timeout(None);
                        if incoming_tx.send(HostToWindow::Ready { version }).is_err() {
                            let _ = reader_outgoing.send(Outgoing::Stop);
                            let _ = reader.get_ref().shutdown(std::net::Shutdown::Both);
                            return;
                        }
                    }
                    Ok(Some(HostToWindow::Rejected { reason })) => {
                        let _ = incoming_tx.send(HostToWindow::Rejected { reason });
                        let _ = reader_outgoing.send(Outgoing::Stop);
                        let _ = reader.get_ref().shutdown(std::net::Shutdown::Both);
                        return;
                    }
                    other => {
                        let _ = incoming_tx.send(HostToWindow::Rejected {
                            reason: format!(
                                "UI handshake failed: {other:?}; restart the complete application"
                            ),
                        });
                        let _ = reader_outgoing.send(Outgoing::Stop);
                        let _ = reader.get_ref().shutdown(std::net::Shutdown::Both);
                        return;
                    }
                }
                loop {
                    match read_frame::<HostToWindow>(&mut reader) {
                        Ok(Some(frame)) => {
                            if incoming_tx.send(frame).is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            log::warn!("[ui-client] host frame error: {error}");
                            break;
                        }
                    }
                }
                let _ = reader_outgoing.send(Outgoing::Stop);
                let _ = reader.get_ref().shutdown(std::net::Shutdown::Both);
            })
            .map_err(|error| format!("UI bridge reader thread failed: {error}"))?;
        let writer = std::thread::Builder::new()
            .name("openless-ui-client-writer".into())
            .spawn(move || {
                let mut stream = stream;
                write_outgoing(&mut stream, outgoing_rx);
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        let writer = match writer {
            Ok(writer) => writer,
            Err(error) => {
                let _ = control.shutdown(std::net::Shutdown::Both);
                let _ = reader.join();
                return Err(format!("UI bridge writer thread failed: {error}"));
            }
        };
        Ok(Self {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            reader: Some(reader),
            writer: Some(writer),
            control,
            ready: false,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn try_recv(&mut self) -> Result<HostToWindow, TryRecvError> {
        let frame = self.incoming.try_recv()?;
        if matches!(
            frame,
            HostToWindow::Ready {
                version: UI_BRIDGE_VERSION
            }
        ) {
            self.ready = true;
        }
        Ok(frame)
    }

    pub fn send(&mut self, frame: WindowToHost) -> Result<(), String> {
        if !self.ready {
            return Err("UI handshake is not ready".into());
        }
        self.outgoing
            .send(Outgoing::ClientFrame(frame))
            .map_err(|_| "UI bridge is closed".into())
    }

    /// 退出前收尾：停掉读写线程。
    pub fn shutdown(&mut self) {
        let _ = self.outgoing.send(Outgoing::Stop);
        std::thread::sleep(Duration::from_millis(20));
        // 关 socket 才能让阻塞在 read 上的读线程退出。
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for UiBridgeClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::frontend::view_model::Page;

    /// 轮询等待一个非空结果（跨线程的帧到达有延迟，不能在测试里假设「立刻」）。
    fn wait_for<T>(mut poll: impl FnMut() -> Option<T>) -> T {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(value) = poll() {
                return value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for a frame"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openless-ui-bridge-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn frames_round_trip_over_the_socket() {
        let dir = temp_dir("roundtrip");
        let path = ui_socket_path(&dir);
        let mut host = UiBridgeHost::bind(path.clone()).unwrap();
        let mut client = UiBridgeClient::connect(&path).unwrap();
        host.accept_pending();
        assert!(!host.is_connected());
        let received = wait_for(|| {
            let messages = host.drain();
            (!messages.is_empty()).then_some(messages)
        });
        assert!(matches!(received.as_slice(), [WindowToHost::Hello { .. }]));
        assert!(matches!(
            wait_for(|| client.try_recv().ok()),
            HostToWindow::Ready {
                version: UI_BRIDGE_VERSION
            }
        ));
        assert!(host.is_connected());

        let view_model = FrontendViewModel {
            active_page: Page::History,
            ..Default::default()
        };
        let payload = serde_json::to_vec(&view_model).unwrap();
        host.send_snapshot_encoded(&payload);
        let frame = loop {
            match client.try_recv() {
                Ok(frame) => break frame,
                Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(10)),
                Err(TryRecvError::Disconnected) => panic!("client disconnected early"),
            }
        };
        match frame {
            HostToWindow::Snapshot {
                sequence,
                view_model,
            } => {
                assert_eq!(sequence, 1, "first snapshot is sequence 1");
                assert_eq!(view_model.active_page, Page::History);
            }
            other => panic!("unexpected frame {other:?}"),
        }
        host.shutdown();
        client.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshots_are_monotonic_and_stale_frames_are_dropped() {
        let dir = temp_dir("monotonic");
        let path = ui_socket_path(&dir);
        let mut host = UiBridgeHost::bind(path.clone()).unwrap();
        let mut client = UiBridgeClient::connect(&path).unwrap();
        host.accept_pending();
        wait_for(|| {
            host.drain();
            host.is_connected().then_some(())
        });
        assert!(matches!(
            wait_for(|| client.try_recv().ok()),
            HostToWindow::Ready { .. }
        ));
        for _ in 0..3 {
            let payload = serde_json::to_vec(&FrontendViewModel::default()).unwrap();
            host.send_snapshot_encoded(&payload);
        }
        let mut sequences = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        // 连发三份快照：写线程可能合并掉中间那些（UI 卡顿时宁可跳帧），
        // 但**到达 UI 的序号必须严格递增**，而且最后一份必然是最新的。
        while std::time::Instant::now() < deadline {
            match client.try_recv() {
                Ok(HostToWindow::Snapshot { sequence, .. }) => sequences.push(sequence),
                Ok(_) => {}
                Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(10)),
                Err(TryRecvError::Disconnected) => break,
            }
            if sequences.last().copied() == Some(3) {
                break;
            }
        }
        assert!(
            !sequences.is_empty(),
            "at least the newest snapshot arrives"
        );
        assert_eq!(sequences.last().copied(), Some(3));
        assert!(
            sequences.windows(2).all(|pair| pair[0] < pair[1]),
            "snapshot sequences must never go backwards: {sequences:?}"
        );
        host.shutdown();
        client.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_bridge_socket_is_replaced_on_bind() {
        let dir = temp_dir("stale");
        let path = ui_socket_path(&dir);
        std::fs::write(&path, b"junk").unwrap();
        let host = UiBridgeHost::bind(path.clone()).unwrap();
        assert_eq!(host.path(), path.as_path());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn handshake_rejection_timeout_and_reconnect_release_the_slot() {
        let dir = temp_dir("handshake");
        let path = ui_socket_path(&dir);
        let mut host = UiBridgeHost::bind(path.clone()).unwrap();
        for frame in [
            WindowToHost::Hello {
                version: UI_BRIDGE_VERSION - 1,
            },
            WindowToHost::Action {
                sequence: 1,
                action: Box::new(FrontendAction::HistoryRefresh),
            },
        ] {
            let mut raw = UnixStream::connect(&path).unwrap();
            raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            host.accept_pending();
            host.send_snapshot_encoded(b"{}");
            assert!(!host.is_connected());
            write_frame(&mut raw, &frame).unwrap();
            wait_for(|| {
                assert!(host.drain().is_empty());
                host.connection.is_none().then_some(())
            });
            assert!(matches!(
                read_frame::<HostToWindow>(&mut BufReader::new(raw)).unwrap(),
                Some(HostToWindow::Rejected { .. })
            ));
        }
        let idle = UnixStream::connect(&path).unwrap();
        host.accept_pending();
        host.connection.as_mut().unwrap().deadline = std::time::Instant::now();
        assert!(host.drain().is_empty());
        assert!(host.connection.is_none());
        drop(idle);
        for _ in 0..2 {
            let mut client = UiBridgeClient::connect(&path).unwrap();
            assert!(client.send(WindowToHost::Ping { sequence: 1 }).is_err());
            host.accept_pending();
            wait_for(|| {
                host.drain();
                host.is_connected().then_some(())
            });
            assert!(matches!(
                wait_for(|| client.try_recv().ok()),
                HostToWindow::Ready { .. }
            ));
            drop(client);
            wait_for(|| {
                host.drain();
                host.connection.is_none().then_some(())
            });
        }
    }

    #[test]
    fn action_and_hotkey_sequences_are_independent_and_reset_per_connection() {
        let dir = temp_dir("sequences");
        let path = ui_socket_path(&dir);
        let mut host = UiBridgeHost::bind(path.clone()).unwrap();
        for _ in 0..2 {
            let mut client = UiBridgeClient::connect(&path).unwrap();
            host.accept_pending();
            wait_for(|| {
                host.drain();
                host.is_connected().then_some(())
            });
            wait_for(|| client.try_recv().ok());
            for sequence in [2, 2, 1, 3] {
                client
                    .send(WindowToHost::Action {
                        sequence,
                        action: Box::new(FrontendAction::HistoryRefresh),
                    })
                    .unwrap();
                client
                    .send(WindowToHost::Hotkey {
                        sequence,
                        edge: openless_linux_egui::LocalHotkeyEdge {
                            hotkey: openless_linux_egui::LocalHotkey::OpenApp,
                            kind: openless_linux_egui::LocalHotkeyEdgeKind::Pressed,
                            press_id: sequence,
                        },
                    })
                    .unwrap();
            }
            client.send(WindowToHost::Ping { sequence: 99 }).unwrap();
            let mut messages = Vec::new();
            wait_for(|| {
                messages.extend(host.drain());
                messages
                    .iter()
                    .any(|message| matches!(message, WindowToHost::Ping { sequence: 99 }))
                    .then_some(())
            });
            assert_eq!(
                messages
                    .iter()
                    .filter(|message| matches!(message, WindowToHost::Action { .. }))
                    .count(),
                2
            );
            assert_eq!(
                messages
                    .iter()
                    .filter(|message| matches!(message, WindowToHost::Hotkey { .. }))
                    .count(),
                2
            );
            drop(client);
            wait_for(|| {
                host.drain();
                host.connection.is_none().then_some(())
            });
        }
    }

    #[test]
    fn frame_limits_apply_before_an_unterminated_line_is_fully_read() {
        let bytes = vec![b'x'; MAX_FRAME_BYTES * 2];
        let mut reader = std::io::Cursor::new(bytes);
        assert!(read_frame::<WindowToHost>(&mut reader).is_err());
        assert!(reader.position() <= (MAX_FRAME_BYTES + 1) as u64);
        assert!(write_frame(&mut Vec::new(), &"x".repeat(MAX_FRAME_BYTES)).is_err());
        let (mut writer, _reader) = UnixStream::pair().unwrap();
        assert!(Outgoing::Encoded(vec![b'x'; MAX_FRAME_BYTES + 1])
            .write_to(&mut writer)
            .is_err());
    }
    #[test]
    fn control_frames_precede_and_shutdown_discards_the_unsent_snapshot() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();
        tx.send(Outgoing::Encoded(b"discarded snapshot".to_vec()))
            .unwrap();
        tx.send(Outgoing::Frame(HostToWindow::FocusMain)).unwrap();
        tx.send(Outgoing::Frame(HostToWindow::Shutdown)).unwrap();
        write_outgoing(&mut writer, rx);
        drop(writer);
        let mut reader = BufReader::new(reader);
        assert!(matches!(
            read_frame::<HostToWindow>(&mut reader).unwrap(),
            Some(HostToWindow::FocusMain)
        ));
        assert!(matches!(
            read_frame::<HostToWindow>(&mut reader).unwrap(),
            Some(HostToWindow::Shutdown)
        ));
        assert!(read_frame::<HostToWindow>(&mut reader).unwrap().is_none());
    }
    #[test]
    fn eof_stops_both_client_workers_before_the_client_is_dropped() {
        let dir = temp_dir("eof-workers");
        let path = ui_socket_path(&dir);
        let listener = UnixListener::bind(&path).unwrap();
        let client = UiBridgeClient::connect(&path).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        write_frame(
            &mut stream,
            &HostToWindow::Ready {
                version: UI_BRIDGE_VERSION,
            },
        )
        .unwrap();
        drop(stream);
        wait_for(|| {
            (client.reader.as_ref().unwrap().is_finished()
                && client.writer.as_ref().unwrap().is_finished())
            .then_some(())
        });
    }
}
