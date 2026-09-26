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
pub const UI_BRIDGE_VERSION: u32 = 1;

/// 单帧上限。视图模型快照含历史列表，比弹窗协议大得多。
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// UI 进程等待宿主 socket 出现的上限（宿主 bind 后才拉它，正常是毫秒级）。
pub const UI_CLIENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostToWindow {
    /// 握手确认：UI 收到它才算真的接上，此前不渲染任何业务数据。
    Ready { version: u32 },
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
    Pong { sequence: u64 },
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
        action: FrontendAction,
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
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if read > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "UI bridge frame too large",
        ));
    }
    let frame = serde_json::from_str(line.trim_end())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok(Some(frame))
}

/// 一个已连上的 UI 窗口。读线程负责 `incoming`，写线程负责 `outgoing`。
struct UiConnection {
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

    fn is_stop(&self) -> bool {
        matches!(self, Outgoing::Stop)
    }

    fn write_to(&self, stream: &mut UnixStream) -> std::io::Result<()> {
        match self {
            Outgoing::Stop => Ok(()),
            Outgoing::Frame(frame) => write_frame(stream, frame),
            Outgoing::ClientFrame(frame) => write_frame(stream, frame),
            Outgoing::Encoded(bytes) => {
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
        self.connection.is_some()
    }

    /// 取走 UI 发来的消息（非阻塞，保持到达顺序）。
    pub fn drain(&mut self) -> Vec<WindowToHost> {
        let mut messages = Vec::new();
        let mut disconnected = false;
        if let Some(connection) = self.connection.as_ref() {
            loop {
                match connection.incoming.try_recv() {
                    Ok(message) => messages.push(message),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            log::info!("[ui-host] UI window disconnected");
            self.connection = None;
        }
        messages
    }

    /// 发送快照，`payload` 必须是 `view_model` 的 JSON 编码。
    ///
    /// 宿主为了判断「视图模型变没变」已经序列化过一次，这里直接拼帧、不再二次
    /// 序列化；`sequence` 由桥推进，保证单调递增。
    pub fn send_snapshot_encoded(&mut self, payload: &[u8]) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let mut frame = Vec::with_capacity(payload.len() + 64);
        frame.extend_from_slice(b"{\"Snapshot\":{\"sequence\":");
        frame.extend_from_slice(sequence.to_string().as_bytes());
        frame.extend_from_slice(b",\"view_model\":");
        frame.extend_from_slice(payload);
        frame.extend_from_slice(b"}}");
        if let Some(connection) = self.connection.as_ref() {
            if connection.outgoing.send(Outgoing::Encoded(frame)).is_err() {
                self.connection = None;
            }
        }
    }

    pub fn send(&mut self, frame: HostToWindow) {
        if let Some(connection) = self.connection.as_ref() {
            if connection.outgoing.send(Outgoing::Frame(frame)).is_err() {
                self.connection = None;
            }
        }
    }

    /// 宿主退出前的收尾：通知 UI 关窗，然后断开。
    pub fn shutdown(&mut self) {
        if let Some(connection) = self.connection.take() {
            let _ = connection
                .outgoing
                .send(Outgoing::Frame(HostToWindow::Shutdown));
            // 给写线程一点时间把 Shutdown 交给 socket，再让它收尾。
            std::thread::sleep(Duration::from_millis(50));
            let _ = connection.outgoing.send(Outgoing::Stop);
            // 先把 socket 关掉：读线程阻塞在 read 上，只有关闭才能让它退出。
            let _ = connection.control.shutdown(std::net::Shutdown::Both);
            if let Some(writer) = connection.writer {
                let _ = writer.join();
            }
            if let Some(reader) = connection.reader {
                let _ = reader.join();
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn spawn_connection(stream: UnixStream) -> std::io::Result<UiConnection> {
    let reader_stream = stream.try_clone()?;
    let control = stream.try_clone()?;
    let (incoming_tx, incoming_rx) = mpsc::channel();
    let (outgoing_tx, outgoing_rx) = mpsc::channel::<Outgoing>();

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
        })?;

    let writer = std::thread::Builder::new()
        .name("openless-ui-host-writer".into())
        .spawn(move || {
            let mut stream = stream;
            // 待发快照只保留最新一份：UI 慢的时候宁可跳帧，也不能画过期状态。
            let mut pending: Option<Outgoing> = None;
            loop {
                let received = outgoing_rx.recv_timeout(Duration::from_millis(20));
                match received {
                    Ok(stop) if stop.is_stop() => {
                        if let Some(stale) = pending.take() {
                            let _ = stale.write_to(&mut stream);
                        }
                        return;
                    }
                    Ok(outgoing) if outgoing.is_snapshot() => {
                        // 快照可合并：只留最新一份，UI 慢时宁可跳帧也不画过期状态。
                        pending = Some(outgoing);
                    }
                    Ok(outgoing) => {
                        // 控制帧（Ready/Pong/Shutdown）必须先于任何待发快照落盘。
                        if let Some(stale) = pending.take() {
                            if stale.write_to(&mut stream).is_err() {
                                return;
                            }
                        }
                        if outgoing.write_to(&mut stream).is_err() {
                            return;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(stale) = pending.take() {
                            if stale.write_to(&mut stream).is_err() {
                                return;
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        })?;

    Ok(UiConnection {
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
    consecutive_failures: u32,
}

impl UiBridgeClient {
    /// 连宿主的 socket。宿主 bind 之后才拉 UI，所以这里通常一次就成；
    /// 仍做重试以覆盖「宿主刚 bind 就被调度器换出」的竞态。
    pub fn connect(path: &Path) -> Result<Self, String> {
        let deadline = std::time::Instant::now() + UI_CLIENT_CONNECT_TIMEOUT;
        let stream = loop {
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
        let reader_stream = stream
            .try_clone()
            .map_err(|error| format!("UI bridge clone failed: {error}"))?;
        let control = stream
            .try_clone()
            .map_err(|error| format!("UI bridge clone failed: {error}"))?;
        let (incoming_tx, incoming_rx) = mpsc::channel();
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<Outgoing>();
        let reader = std::thread::Builder::new()
            .name("openless-ui-client-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(reader_stream);
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
            })
            .map_err(|error| format!("UI bridge reader thread failed: {error}"))?;
        let writer = std::thread::Builder::new()
            .name("openless-ui-client-writer".into())
            .spawn(move || {
                let mut stream = stream;
                while let Ok(outgoing) = outgoing_rx.recv() {
                    if outgoing.is_snapshot() {
                        continue;
                    }
                    match outgoing {
                        Outgoing::Stop => return,
                        other => {
                            if other.write_to(&mut stream).is_err() {
                                return;
                            }
                        }
                    }
                }
            })
            .map_err(|error| format!("UI bridge writer thread failed: {error}"))?;
        Ok(Self {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            reader: Some(reader),
            writer: Some(writer),
            control,
            consecutive_failures: 0,
        })
    }

    pub fn try_recv(&self) -> Result<HostToWindow, TryRecvError> {
        self.incoming.try_recv()
    }

    /// 发一帧；失败累计到阈值就报错，让 UI 进程知道宿主已经走了。
    pub fn send(&mut self, frame: WindowToHost) -> Result<(), String> {
        match self.outgoing.send(Outgoing::ClientFrame(frame)) {
            Ok(()) => {
                self.consecutive_failures = 0;
                Ok(())
            }
            Err(_) => {
                self.consecutive_failures += 1;
                Err("UI bridge is closed".to_string())
            }
        }
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
        assert!(host.is_connected());

        client
            .send(WindowToHost::Hello {
                version: UI_BRIDGE_VERSION,
            })
            .unwrap();
        // 帧要经写线程落到 socket、再经读线程回到宿主，所以轮询等一小会儿。
        let received = wait_for(|| {
            let messages = host.drain();
            if messages.is_empty() {
                None
            } else {
                Some(messages)
            }
        });
        assert!(matches!(received.as_slice(), [WindowToHost::Hello { .. }]));

        let mut view_model = FrontendViewModel::default();
        view_model.active_page = Page::History;
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
}
