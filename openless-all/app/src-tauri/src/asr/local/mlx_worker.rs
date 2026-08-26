//! MLX Qwen3-ASR 隔离进程与本地 IPC。
//!
//! mlx-c 的致命错误会直接结束当前进程，因此所有 MLX 初始化、加载和推理都放在
//! 当前可执行文件的隐藏 worker 模式中。主进程只通过 Unix socket 发送小型 JSON 帧。

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use qwen3_asr_rs::inference::AsrInference;
use qwen3_asr_rs::tensor::Device;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::mlx_qwen_engine::ensure_tokenizer_json;

const WORKER_ARGUMENT: &str = "--openless-mlx-worker";
const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const DIAGNOSTIC_TAIL_BYTES: usize = 64 * 1024;
const START_TIMEOUT: Duration = Duration::from_secs(10);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const LOAD_POLL_INTERVAL: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkerPhase {
    WorkerStart,
    Handshake,
    ModelValidation,
    MlxInitialization,
    ModelLoad,
    Inference,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MlxErrorCode {
    WorkerStart,
    ProtocolMismatch,
    ModelValidation,
    MlxInitialization,
    ModelLoad,
    OperatorIncompatible,
    Inference,
    AllocationFailure,
    NativeProcessExit,
    Timeout,
    Busy,
    Cancelled,
    Io,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "PascalCase",
    rename_all_fields = "camelCase"
)]
enum WorkerRequest {
    Hello {
        request_id: u64,
        protocol_version: u32,
    },
    Load {
        request_id: u64,
        model_dir: String,
    },
    Transcribe {
        request_id: u64,
        wav_path: String,
    },
    Shutdown {
        request_id: u64,
    },
}

impl WorkerRequest {
    fn request_id(&self) -> u64 {
        match self {
            Self::Hello { request_id, .. }
            | Self::Load { request_id, .. }
            | Self::Transcribe { request_id, .. }
            | Self::Shutdown { request_id } => *request_id,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "PascalCase",
    rename_all_fields = "camelCase"
)]
enum WorkerResponse {
    HelloAck {
        request_id: u64,
        protocol_version: u32,
        worker_pid: u32,
    },
    Progress {
        request_id: u64,
        phase: WorkerPhase,
    },
    Loaded {
        request_id: u64,
    },
    Transcript {
        request_id: u64,
        text: String,
    },
    Error {
        request_id: u64,
        phase: WorkerPhase,
        code: MlxErrorCode,
        message: String,
    },
}

impl WorkerResponse {
    fn request_id(&self) -> u64 {
        match self {
            Self::HelloAck { request_id, .. }
            | Self::Progress { request_id, .. }
            | Self::Loaded { request_id }
            | Self::Transcript { request_id, .. }
            | Self::Error { request_id, .. } => *request_id,
        }
    }
}

#[derive(Clone, Default)]
struct TailBuffer(Arc<Mutex<Vec<u8>>>);

impl TailBuffer {
    fn append(&self, bytes: &[u8]) {
        let Ok(mut tail) = self.0.lock() else {
            return;
        };
        if bytes.len() >= DIAGNOSTIC_TAIL_BYTES {
            tail.clear();
            tail.extend_from_slice(&bytes[bytes.len() - DIAGNOSTIC_TAIL_BYTES..]);
            return;
        }
        let excess = tail
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(DIAGNOSTIC_TAIL_BYTES);
        if excess > 0 {
            tail.drain(..excess);
        }
        tail.extend_from_slice(bytes);
    }

    fn text(&self) -> String {
        self.0
            .lock()
            .map(|tail| String::from_utf8_lossy(&tail).into_owned())
            .unwrap_or_else(|_| "<diagnostic buffer poisoned>".to_string())
    }
}

#[derive(Clone, Default)]
struct Diagnostics {
    stdout: TailBuffer,
    stderr: TailBuffer,
}

impl Diagnostics {
    fn text(&self) -> String {
        format!(
            "stdout tail:\n{}\nstderr tail:\n{}",
            self.stdout.text(),
            self.stderr.text()
        )
    }
}

fn spawn_capture<R>(mut reader: R, tail: TailBuffer) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => return,
                Ok(count) => tail.append(&buffer[..count]),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return,
            }
        }
    })
}

fn write_frame<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value).context("serialize MLX worker frame")?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        anyhow::bail!(
            "MLX worker frame size {} exceeds limit {}",
            payload.len(),
            MAX_FRAME_BYTES
        );
    }
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .context("write MLX worker frame header")?;
    stream
        .write_all(&payload)
        .context("write MLX worker frame body")?;
    stream.flush().context("flush MLX worker frame")?;
    Ok(())
}

fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<Option<T>> {
    let mut header = [0_u8; 4];
    let first = loop {
        match stream.read(&mut header[..1]) {
            Ok(count) => break count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("read MLX worker frame header"),
        }
    };
    if first == 0 {
        return Ok(None);
    }
    stream
        .read_exact(&mut header[1..])
        .context("truncated MLX worker frame header")?;
    let size = u32::from_be_bytes(header) as usize;
    if size == 0 || size > MAX_FRAME_BYTES {
        anyhow::bail!("invalid MLX worker frame size: {size}");
    }
    let mut payload = vec![0_u8; size];
    stream
        .read_exact(&mut payload)
        .context("truncated MLX worker frame body")?;
    serde_json::from_slice(&payload)
        .context("decode MLX worker frame")
        .map(Some)
}

fn classify_error_message(message: &str, fallback: MlxErrorCode) -> MlxErrorCode {
    let lower = message.to_ascii_lowercase();
    let has_oom_marker = lower
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| token == "oom");
    if has_oom_marker
        || [
            "out of memory",
            "memory allocation failed",
            "failed to allocate",
            "cannot allocate memory",
            "std::bad_alloc",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return MlxErrorCode::AllocationFailure;
    }
    if [
        "unsupported operator",
        "unsupported operation",
        "no kernel for",
        "shape mismatch",
        "unsupported shape",
        "incompatible shape",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return MlxErrorCode::OperatorIncompatible;
    }
    fallback
}

fn user_error(code: MlxErrorCode) -> anyhow::Error {
    let message = match code {
        MlxErrorCode::AllocationFailure => "MLX/Metal 内存分配失败",
        MlxErrorCode::OperatorIncompatible => "MLX/Metal 算子或模型结构不兼容",
        MlxErrorCode::ModelValidation => "Qwen3-ASR MLX 模型文件校验失败",
        MlxErrorCode::MlxInitialization => "MLX/Metal 初始化失败",
        MlxErrorCode::ModelLoad => "Qwen3-ASR MLX 模型加载失败",
        MlxErrorCode::Inference => "Qwen3-ASR MLX 解码失败",
        MlxErrorCode::Timeout => "MLX worker 操作超时",
        MlxErrorCode::ProtocolMismatch => "MLX worker 协议不兼容",
        MlxErrorCode::WorkerStart => "MLX worker 启动失败",
        MlxErrorCode::NativeProcessExit => "MLX worker 异常退出",
        MlxErrorCode::Io => "MLX worker 通信失败",
        MlxErrorCode::Busy => "MLX worker 正在处理另一个转写请求",
        MlxErrorCode::Cancelled => "MLX worker 转写已取消",
    };
    anyhow::anyhow!(message)
}

pub(super) struct MlxWorkerClient {
    session_dir: PathBuf,
    io: Mutex<UnixStream>,
    control: UnixStream,
    child: Mutex<Option<Child>>,
    last_exit_status: Mutex<Option<String>>,
    healthy: AtomicBool,
    next_request_id: AtomicU64,
    next_operation_id: AtomicU64,
    active_operation: Mutex<Option<u64>>,
    last_phase: Mutex<WorkerPhase>,
    diagnostics: Diagnostics,
    capture_threads: Mutex<Vec<JoinHandle<()>>>,
}

struct ActiveOperationGuard<'a> {
    active_operation: &'a Mutex<Option<u64>>,
    operation_id: u64,
}

impl Drop for ActiveOperationGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active_operation.lock() {
            if *active == Some(self.operation_id) {
                *active = None;
            }
        }
    }
}

impl MlxWorkerClient {
    pub(super) fn load(model_dir: &Path) -> Result<Self> {
        let started_at = Instant::now();
        let worker_start_failure = |detail: String| {
            log::error!(
                "[local-qwen3-mlx] worker start failure phase={:?} code={:?}: {detail}",
                WorkerPhase::WorkerStart,
                MlxErrorCode::WorkerStart
            );
            user_error(MlxErrorCode::WorkerStart)
        };
        // macOS 的 TMPDIR 通常位于很深的 /var/folders 路径；Unix socket 的
        // sun_path 只有 104 字节。使用系统短临时根目录，私有性仍由唯一目录和
        // 0700 权限保证。
        let session_dir = Path::new("/tmp").join(format!(
            "openless-mlx-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&session_dir)
            .map_err(|error| {
                worker_start_failure(format!(
                    "create private session directory {}: {error}",
                    session_dir.display()
                ))
            })?;
        let mut session_guard = SessionDirGuard::new(session_dir.clone());
        fs::set_permissions(&session_dir, fs::Permissions::from_mode(0o700)).map_err(|error| {
            worker_start_failure(format!(
                "set private session permissions {}: {error}",
                session_dir.display()
            ))
        })?;
        let socket_path = session_dir.join("worker.sock");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = fs::remove_dir_all(&session_dir);
                return Err(worker_start_failure(format!(
                    "bind worker socket {}: {error}",
                    socket_path.display()
                )));
            }
        };
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            worker_start_failure(format!(
                "set worker socket permissions {}: {error}",
                socket_path.display()
            ))
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            worker_start_failure(format!("set worker socket nonblocking: {error}"))
        })?;

        let executable = std::env::current_exe().map_err(|error| {
            worker_start_failure(format!("resolve OpenLess executable: {error}"))
        })?;
        let mut child = match Command::new(executable)
            .arg(WORKER_ARGUMENT)
            .arg(&socket_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&socket_path);
                let _ = fs::remove_dir_all(&session_dir);
                return Err(worker_start_failure(format!(
                    "spawn MLX worker process: {error}"
                )));
            }
        };

        let diagnostics = Diagnostics::default();
        let mut capture_threads = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            capture_threads.push(spawn_capture(stdout, diagnostics.stdout.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            capture_threads.push(spawn_capture(stderr, diagnostics.stderr.clone()));
        }

        let stream = match accept_worker(&listener, &mut child, started_at, &diagnostics) {
            Ok(stream) => stream,
            Err(error) => {
                terminate_unmanaged_child(&mut child);
                for thread in capture_threads {
                    let _ = thread.join();
                }
                let _ = fs::remove_file(&socket_path);
                let _ = fs::remove_dir_all(&session_dir);
                return Err(error);
            }
        };
        let _ = fs::remove_file(&socket_path);
        let control = match stream.try_clone() {
            Ok(control) => control,
            Err(error) => {
                terminate_unmanaged_child(&mut child);
                for thread in capture_threads {
                    let _ = thread.join();
                }
                let _ = fs::remove_dir_all(&session_dir);
                return Err(worker_start_failure(format!(
                    "clone MLX worker control socket: {error}\n{}",
                    diagnostics.text()
                )));
            }
        };
        session_guard.disarm();
        let client = Self {
            session_dir,
            io: Mutex::new(stream),
            control,
            child: Mutex::new(Some(child)),
            last_exit_status: Mutex::new(None),
            healthy: AtomicBool::new(true),
            next_request_id: AtomicU64::new(1),
            next_operation_id: AtomicU64::new(1),
            active_operation: Mutex::new(None),
            last_phase: Mutex::new(WorkerPhase::Handshake),
            diagnostics,
            capture_threads: Mutex::new(capture_threads),
        };

        let remaining = START_TIMEOUT.saturating_sub(started_at.elapsed());
        if remaining.is_zero() {
            client.abort();
            return Err(user_error(MlxErrorCode::Timeout));
        }
        if let Err(error) = client.handshake(remaining) {
            client.abort();
            return Err(error);
        }
        if let Err(error) = client.load_model(model_dir) {
            client.abort();
            return Err(error);
        }
        Ok(client)
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn next_operation_id(&self) -> u64 {
        self.next_operation_id.fetch_add(1, Ordering::Relaxed)
    }

    fn handshake(&self, timeout: Duration) -> Result<()> {
        let request_id = self.next_request_id();
        let mut stream = self.io.lock().map_err(|_| user_error(MlxErrorCode::Io))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        write_frame(
            &mut stream,
            &WorkerRequest::Hello {
                request_id,
                protocol_version: PROTOCOL_VERSION,
            },
        )
        .map_err(|error| self.io_failure(error, WorkerPhase::Handshake))?;
        let response = read_frame::<WorkerResponse>(&mut stream)
            .map_err(|error| self.io_failure(error, WorkerPhase::Handshake))?
            .ok_or_else(|| self.transport_failure(WorkerPhase::Handshake, "socket EOF"))?;
        validate_request_id(request_id, &response).map_err(|error| {
            self.protocol_failure(WorkerPhase::Handshake, &format!("{error:#}"))
        })?;
        match response {
            WorkerResponse::HelloAck {
                protocol_version, ..
            } if protocol_version == PROTOCOL_VERSION => Ok(()),
            WorkerResponse::Error {
                phase,
                code,
                message,
                ..
            } => Err(self.response_error(phase, code, &message)),
            _ => Err(self.protocol_failure(WorkerPhase::Handshake, "unexpected hello response")),
        }
    }

    fn load_model(&self, model_dir: &Path) -> Result<()> {
        let model_dir = model_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Qwen3-ASR 模型路径不是有效 UTF-8"))?;
        let load_started = Instant::now();
        let request_id = self.next_request_id();
        let mut stream = self.io.lock().map_err(|_| user_error(MlxErrorCode::Io))?;
        // 使用固定短轮询保持 120 秒总截止时间，同时避免 macOS 在 peer 已关闭后
        // 再次 setsockopt(SO_RCVTIMEO) 返回 EINVAL，掩盖真正的 worker EOF。
        stream.set_read_timeout(Some(LOAD_POLL_INTERVAL))?;
        stream.set_write_timeout(Some(START_TIMEOUT))?;
        write_frame(
            &mut stream,
            &WorkerRequest::Load {
                request_id,
                model_dir: model_dir.to_string(),
            },
        )
        .map_err(|error| self.io_failure(error, WorkerPhase::ModelLoad))?;
        loop {
            if load_started.elapsed() >= LOAD_TIMEOUT {
                return Err(self.timeout_failure(WorkerPhase::ModelLoad));
            }
            let response = match read_frame::<WorkerResponse>(&mut stream) {
                Ok(Some(response)) => response,
                Ok(None) => {
                    return Err(self.transport_failure(WorkerPhase::ModelLoad, "socket EOF"));
                }
                Err(error)
                    if error.downcast_ref::<std::io::Error>().is_some_and(|io| {
                        matches!(io.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
                    }) =>
                {
                    continue;
                }
                Err(error) => return Err(self.io_failure(error, WorkerPhase::ModelLoad)),
            };
            validate_request_id(request_id, &response).map_err(|error| {
                self.protocol_failure(WorkerPhase::ModelLoad, &format!("{error:#}"))
            })?;
            match response {
                WorkerResponse::Progress { phase, .. } => self.set_phase(phase),
                WorkerResponse::Loaded { .. } => {
                    stream.set_read_timeout(None)?;
                    return Ok(());
                }
                WorkerResponse::Error {
                    phase,
                    code,
                    message,
                    ..
                } => return Err(self.response_error(phase, code, &message)),
                _ => {
                    return Err(
                        self.protocol_failure(WorkerPhase::ModelLoad, "unexpected load response")
                    );
                }
            }
        }
    }

    pub(super) fn transcribe_pcm(&self, samples: &[f32]) -> Result<String> {
        let cancelled = AtomicBool::new(false);
        self.transcribe_pcm_for_operation(self.next_operation_id(), samples, &cancelled)
    }

    pub(super) fn transcribe_pcm_for_operation(
        &self,
        operation_id: u64,
        samples: &[f32],
        cancelled: &AtomicBool,
    ) -> Result<String> {
        let _operation = self.claim_operation(operation_id, cancelled)?;
        if !self.is_healthy() {
            return Err(user_error(MlxErrorCode::NativeProcessExit));
        }
        let wav = TempWav::new(&self.session_dir, samples)?;
        let wav_path = wav
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("MLX 临时 WAV 路径不是有效 UTF-8"))?
            .to_string();
        let request_id = self.next_request_id();
        let mut stream = self.io.lock().map_err(|_| user_error(MlxErrorCode::Io))?;
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(Some(START_TIMEOUT))?;
        write_frame(
            &mut stream,
            &WorkerRequest::Transcribe {
                request_id,
                wav_path,
            },
        )
        .map_err(|error| self.io_failure(error, WorkerPhase::Inference))?;
        loop {
            let response = read_frame::<WorkerResponse>(&mut stream)
                .map_err(|error| self.io_failure(error, WorkerPhase::Inference))?
                .ok_or_else(|| self.transport_failure(WorkerPhase::Inference, "socket EOF"))?;
            validate_request_id(request_id, &response).map_err(|error| {
                self.protocol_failure(WorkerPhase::Inference, &format!("{error:#}"))
            })?;
            match response {
                WorkerResponse::Progress { phase, .. } => self.set_phase(phase),
                WorkerResponse::Transcript { text, .. } => return Ok(text),
                WorkerResponse::Error {
                    phase,
                    code,
                    message,
                    ..
                } => return Err(self.response_error(phase, code, &message)),
                _ => {
                    return Err(self.protocol_failure(
                        WorkerPhase::Inference,
                        "unexpected transcribe response",
                    ));
                }
            }
        }
    }

    fn claim_operation(
        &self,
        operation_id: u64,
        cancelled: &AtomicBool,
    ) -> Result<ActiveOperationGuard<'_>> {
        // cancel() 先置 cancelled，再取同一把短锁检查 owner。这里在锁内同时检查
        // 标志并登记 owner，封住“已取消但 blocking task 尚未开始”的竞态。
        let mut active = self
            .active_operation
            .lock()
            .map_err(|_| user_error(MlxErrorCode::Io))?;
        if cancelled.load(Ordering::Acquire) {
            return Err(user_error(MlxErrorCode::Cancelled));
        }
        if active.is_some() {
            return Err(user_error(MlxErrorCode::Busy));
        }
        *active = Some(operation_id);
        Ok(ActiveOperationGuard {
            active_operation: &self.active_operation,
            operation_id,
        })
    }

    pub(super) fn cancel_operation(&self, operation_id: u64) {
        let owns_worker = self
            .active_operation
            .lock()
            .map(|active| *active == Some(operation_id))
            .unwrap_or(false);
        if owns_worker {
            self.abort();
        }
    }

    pub(super) fn abort(&self) {
        self.healthy.store(false, Ordering::Release);
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        self.stop_child(Duration::ZERO);
    }

    pub(super) fn is_healthy(&self) -> bool {
        if !self.healthy.load(Ordering::Acquire) {
            return false;
        }
        let status = match self.child.lock() {
            Ok(mut slot) => match slot.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(status) => status,
                    Err(_) => {
                        self.healthy.store(false, Ordering::Release);
                        return false;
                    }
                },
                None => {
                    self.healthy.store(false, Ordering::Release);
                    return false;
                }
            },
            Err(_) => {
                self.healthy.store(false, Ordering::Release);
                return false;
            }
        };
        if let Some(status) = status {
            self.healthy.store(false, Ordering::Release);
            self.stop_child(Duration::ZERO);
            log::error!(
                "[local-qwen3-mlx] worker exited while idle phase={:?} status={status}\n{}",
                self.phase(),
                self.diagnostics.text()
            );
            return false;
        }
        true
    }

    fn phase(&self) -> WorkerPhase {
        self.last_phase
            .lock()
            .map(|phase| *phase)
            .unwrap_or(WorkerPhase::WorkerStart)
    }

    fn set_phase(&self, phase: WorkerPhase) {
        if let Ok(mut current) = self.last_phase.lock() {
            *current = phase;
        }
    }

    fn response_error(
        &self,
        phase: WorkerPhase,
        code: MlxErrorCode,
        message: &str,
    ) -> anyhow::Error {
        let code = classify_error_message(message, code);
        let fatal = matches!(
            code,
            MlxErrorCode::AllocationFailure | MlxErrorCode::OperatorIncompatible
        );
        if fatal {
            self.abort();
        }
        let status = self
            .last_exit_status
            .lock()
            .ok()
            .and_then(|status| status.clone())
            .unwrap_or_else(|| "still running".to_string());
        log::error!(
            "[local-qwen3-mlx] worker error phase={phase:?} code={code:?} status={status}: {message}\n{}",
            self.diagnostics.text()
        );
        user_error(code)
    }

    fn io_failure(&self, error: anyhow::Error, phase: WorkerPhase) -> anyhow::Error {
        let is_timeout = error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| matches!(io.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock));
        if is_timeout {
            return self.timeout_failure(phase);
        }
        self.transport_failure(phase, &format!("{error:#}"))
    }

    fn timeout_failure(&self, phase: WorkerPhase) -> anyhow::Error {
        self.abort();
        let status = self
            .last_exit_status
            .lock()
            .ok()
            .and_then(|status| status.clone())
            .unwrap_or_else(|| "unknown".to_string());
        log::error!(
            "[local-qwen3-mlx] worker timeout phase={phase:?} status={status}\n{}",
            self.diagnostics.text()
        );
        user_error(MlxErrorCode::Timeout)
    }

    fn protocol_failure(&self, phase: WorkerPhase, detail: &str) -> anyhow::Error {
        self.abort();
        let status = self
            .last_exit_status
            .lock()
            .ok()
            .and_then(|status| status.clone())
            .unwrap_or_else(|| "unknown".to_string());
        log::error!(
            "[local-qwen3-mlx] worker protocol failure phase={phase:?} status={status}: {detail}\n{}",
            self.diagnostics.text()
        );
        user_error(MlxErrorCode::ProtocolMismatch)
    }

    fn transport_failure(&self, phase: WorkerPhase, detail: &str) -> anyhow::Error {
        self.healthy.store(false, Ordering::Release);
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        self.stop_child(Duration::from_millis(250));
        let diagnostic = self.diagnostics.text();
        let code = classify_error_message(&diagnostic, MlxErrorCode::NativeProcessExit);
        let status = self
            .last_exit_status
            .lock()
            .ok()
            .and_then(|status| status.clone())
            .unwrap_or_else(|| "unknown".to_string());
        log::error!(
            "[local-qwen3-mlx] worker transport failure phase={phase:?} code={code:?} status={status}: {detail}\n{diagnostic}"
        );
        user_error(code)
    }

    fn stop_child(&self, grace: Duration) {
        let child = self.child.lock().ok().and_then(|mut child| child.take());
        if let Some(mut child) = child {
            let deadline = Instant::now() + grace;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Ok(None) => {
                        let _ = child.kill();
                        break child.wait().ok();
                    }
                    Err(_) => break None,
                }
            };
            self.record_exit_status(status);
        }
        self.join_capture_threads();
    }

    fn record_exit_status(&self, status: Option<ExitStatus>) {
        if let Ok(mut slot) = self.last_exit_status.lock() {
            *slot = status.map(|status| status.to_string());
        }
    }

    fn join_capture_threads(&self) {
        if let Ok(mut threads) = self.capture_threads.lock() {
            for thread in threads.drain(..) {
                let _ = thread.join();
            }
        }
    }

    fn graceful_shutdown(&self) {
        if !self.healthy.swap(false, Ordering::AcqRel) {
            self.stop_child(Duration::ZERO);
            return;
        }
        self.set_phase(WorkerPhase::Shutdown);
        if let Ok(mut stream) = self.io.lock() {
            let _ = stream.set_write_timeout(Some(SHUTDOWN_TIMEOUT));
            let _ = write_frame(
                &mut stream,
                &WorkerRequest::Shutdown {
                    request_id: self.next_request_id(),
                },
            );
        }
        self.stop_child(SHUTDOWN_TIMEOUT);
    }
}

impl Drop for MlxWorkerClient {
    fn drop(&mut self) {
        self.graceful_shutdown();
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        if let Ok(mut threads) = self.capture_threads.lock() {
            for thread in threads.drain(..) {
                let _ = thread.join();
            }
        }
        let _ = fs::remove_dir_all(&self.session_dir);
    }
}

fn accept_worker(
    listener: &UnixListener,
    child: &mut Child,
    started_at: Instant,
    diagnostics: &Diagnostics,
) -> Result<UnixStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => {
                log::error!(
                    "[local-qwen3-mlx] worker accept failure phase={:?} code={:?}: {error}\n{}",
                    WorkerPhase::WorkerStart,
                    MlxErrorCode::Io,
                    diagnostics.text()
                );
                return Err(user_error(MlxErrorCode::Io));
            }
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                log::error!(
                    "[local-qwen3-mlx] worker status failure phase={:?} code={:?}: {error}\n{}",
                    WorkerPhase::WorkerStart,
                    MlxErrorCode::Io,
                    diagnostics.text()
                );
                return Err(user_error(MlxErrorCode::Io));
            }
        };
        if let Some(status) = status {
            log::error!(
                "[local-qwen3-mlx] worker exited during startup status={status}\n{}",
                diagnostics.text()
            );
            return Err(user_error(classify_error_message(
                &diagnostics.text(),
                MlxErrorCode::NativeProcessExit,
            )));
        }
        if started_at.elapsed() >= START_TIMEOUT {
            log::error!(
                "[local-qwen3-mlx] worker start timeout phase={:?} code={:?}\n{}",
                WorkerPhase::WorkerStart,
                MlxErrorCode::Timeout,
                diagnostics.text()
            );
            return Err(user_error(MlxErrorCode::Timeout));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_unmanaged_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn validate_request_id(expected: u64, response: &WorkerResponse) -> Result<()> {
    let actual = response.request_id();
    if actual != expected {
        anyhow::bail!("MLX worker response request id mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

struct TempWav {
    path: PathBuf,
}

struct SessionDirGuard {
    path: PathBuf,
    armed: bool,
}

impl SessionDirGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SessionDirGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

impl TempWav {
    fn new(session_dir: &Path, samples: &[f32]) -> Result<Self> {
        let path = session_dir.join(format!("audio-{}.wav", uuid::Uuid::new_v4()));
        let pcm: Vec<i16> = samples
            .iter()
            .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("创建 MLX 临时 WAV 失败: {}", path.display()))?;
        let wav = Self { path };
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("设置 MLX 临时 WAV 权限失败: {}", wav.path.display()))?;
        file.write_all(&crate::asr::wav::encode_wav_16k_mono(&pcm))
            .with_context(|| format!("写入 MLX 临时 WAV 失败: {}", wav.path.display()))?;
        file.flush()
            .with_context(|| format!("提交 MLX 临时 WAV 失败: {}", wav.path.display()))?;
        Ok(wav)
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn run_if_requested() {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(WORKER_ARGUMENT)) {
        return;
    }
    let Some(socket_path) = args.next().map(PathBuf::from) else {
        eprintln!("{WORKER_ARGUMENT} requires a socket path");
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("{WORKER_ARGUMENT} accepts exactly one socket path");
        std::process::exit(2);
    }
    match run_worker(&socket_path) {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("MLX worker failed: {error:#}");
            std::process::exit(1);
        }
    }
}

fn run_worker(socket_path: &Path) -> Result<()> {
    let stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connect MLX worker socket: {}", socket_path.display()))?;
    let session_dir = socket_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("MLX worker socket has no parent directory"))?
        .canonicalize()
        .context("canonicalize MLX worker session directory")?;
    serve_worker_stream(stream, &session_dir)
}

fn serve_worker_stream(mut stream: UnixStream, session_dir: &Path) -> Result<()> {
    let mut hello_complete = false;
    let mut inference: Option<AsrInference> = None;
    loop {
        let Some(request) = read_frame::<WorkerRequest>(&mut stream)? else {
            return Ok(());
        };
        let request_id = request.request_id();
        match request {
            WorkerRequest::Hello {
                protocol_version, ..
            } if !hello_complete => {
                if protocol_version != PROTOCOL_VERSION {
                    send_worker_error(
                        &mut stream,
                        request_id,
                        WorkerPhase::Handshake,
                        MlxErrorCode::ProtocolMismatch,
                        format!(
                            "protocol version mismatch: expected {PROTOCOL_VERSION}, got {protocol_version}"
                        ),
                    )?;
                    return Ok(());
                }
                hello_complete = true;
                write_frame(
                    &mut stream,
                    &WorkerResponse::HelloAck {
                        request_id,
                        protocol_version: PROTOCOL_VERSION,
                        worker_pid: std::process::id(),
                    },
                )?;
            }
            WorkerRequest::Load { model_dir, .. } if hello_complete && inference.is_none() => {
                send_progress(&mut stream, request_id, WorkerPhase::ModelValidation)?;
                let model_dir = PathBuf::from(model_dir);
                if let Err(error) = ensure_tokenizer_json(&model_dir) {
                    send_worker_error(
                        &mut stream,
                        request_id,
                        WorkerPhase::ModelValidation,
                        MlxErrorCode::ModelValidation,
                        format!("{error:#}"),
                    )?;
                    continue;
                }
                send_progress(&mut stream, request_id, WorkerPhase::MlxInitialization)?;
                qwen3_asr_rs::backend::mlx::stream::init_mlx(true);
                send_progress(&mut stream, request_id, WorkerPhase::ModelLoad)?;
                match AsrInference::load(&model_dir, Device::gpu()) {
                    Ok(loaded) => {
                        inference = Some(loaded);
                        write_frame(&mut stream, &WorkerResponse::Loaded { request_id })?;
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        send_worker_error(
                            &mut stream,
                            request_id,
                            WorkerPhase::ModelLoad,
                            classify_error_message(&message, MlxErrorCode::ModelLoad),
                            message,
                        )?;
                    }
                }
            }
            WorkerRequest::Transcribe { wav_path, .. } if hello_complete && inference.is_some() => {
                let wav_path = match validate_worker_wav(session_dir, &wav_path) {
                    Ok(path) => path,
                    Err(error) => {
                        send_worker_error(
                            &mut stream,
                            request_id,
                            WorkerPhase::Inference,
                            MlxErrorCode::Io,
                            format!("{error:#}"),
                        )?;
                        continue;
                    }
                };
                send_progress(&mut stream, request_id, WorkerPhase::Inference)?;
                let path = wav_path
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("MLX WAV path is not UTF-8"))?;
                let result = inference
                    .as_ref()
                    .expect("guarded by match condition")
                    .transcribe(path, None);
                match result {
                    Ok(output) => write_frame(
                        &mut stream,
                        &WorkerResponse::Transcript {
                            request_id,
                            text: output.text.trim().to_string(),
                        },
                    )?,
                    Err(error) => {
                        let message = format!("{error:#}");
                        send_worker_error(
                            &mut stream,
                            request_id,
                            WorkerPhase::Inference,
                            classify_error_message(&message, MlxErrorCode::Inference),
                            message,
                        )?;
                    }
                }
            }
            WorkerRequest::Shutdown { .. } if hello_complete => return Ok(()),
            _ => {
                send_worker_error(
                    &mut stream,
                    request_id,
                    WorkerPhase::Handshake,
                    MlxErrorCode::ProtocolMismatch,
                    "request is invalid for current worker state".to_string(),
                )?;
            }
        }
    }
}

fn validate_worker_wav(session_dir: &Path, wav_path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(wav_path)
        .canonicalize()
        .with_context(|| format!("canonicalize MLX WAV path: {wav_path}"))?;
    if path.parent() != Some(session_dir)
        || path.extension().and_then(|extension| extension.to_str()) != Some("wav")
    {
        anyhow::bail!("MLX WAV path is outside the private worker session directory");
    }
    Ok(path)
}

fn send_progress(stream: &mut UnixStream, request_id: u64, phase: WorkerPhase) -> Result<()> {
    write_frame(stream, &WorkerResponse::Progress { request_id, phase })
}

fn send_worker_error(
    stream: &mut UnixStream,
    request_id: u64,
    phase: WorkerPhase,
    code: MlxErrorCode,
    message: String,
) -> Result<()> {
    eprintln!("MLX worker error phase={phase:?} code={code:?}: {message}");
    write_frame(
        stream,
        &WorkerResponse::Error {
            request_id,
            phase,
            code,
            message,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn test_dir() -> PathBuf {
        // macOS 的测试 TMPDIR 同样可能超过 Unix socket 的 SUN_LEN。
        let dir = Path::new("/tmp").join(format!(
            "ol-mlx-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn test_client(stream: UnixStream, session_dir: PathBuf, child: Child) -> MlxWorkerClient {
        let control = stream.try_clone().unwrap();
        MlxWorkerClient {
            session_dir,
            io: Mutex::new(stream),
            control,
            child: Mutex::new(Some(child)),
            last_exit_status: Mutex::new(None),
            healthy: AtomicBool::new(true),
            next_request_id: AtomicU64::new(10),
            next_operation_id: AtomicU64::new(1),
            active_operation: Mutex::new(None),
            last_phase: Mutex::new(WorkerPhase::Inference),
            diagnostics: Diagnostics::default(),
            capture_threads: Mutex::new(Vec::new()),
        }
    }

    fn sleeping_child() -> Child {
        Command::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .unwrap()
    }

    #[test]
    fn frame_round_trip_preserves_request() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        let request = WorkerRequest::Hello {
            request_id: 7,
            protocol_version: PROTOCOL_VERSION,
        };
        write_frame(&mut sender, &request).unwrap();
        let decoded = read_frame::<WorkerRequest>(&mut receiver).unwrap().unwrap();
        assert_eq!(decoded.request_id(), 7);
    }

    #[test]
    fn wire_format_uses_versioned_pascal_case_messages_and_camel_case_fields() {
        let value = serde_json::to_value(WorkerRequest::Hello {
            request_id: 7,
            protocol_version: PROTOCOL_VERSION,
        })
        .unwrap();
        assert_eq!(value["type"], "Hello");
        assert_eq!(value["requestId"], 7);
        assert_eq!(value["protocolVersion"], PROTOCOL_VERSION);
        assert!(value.get("request_id").is_none());
    }

    #[test]
    fn oversized_and_truncated_frames_are_rejected() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender
            .write_all(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes())
            .unwrap();
        assert!(read_frame::<WorkerRequest>(&mut receiver).is_err());

        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender.write_all(&5_u32.to_be_bytes()).unwrap();
        sender.write_all(b"{}").unwrap();
        sender.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(read_frame::<WorkerRequest>(&mut receiver).is_err());
    }

    #[test]
    fn response_request_id_must_match() {
        let response = WorkerResponse::Loaded { request_id: 9 };
        assert!(validate_request_id(8, &response).is_err());
    }

    #[test]
    fn explicit_native_messages_are_classified_conservatively() {
        assert_eq!(
            classify_error_message("failed to allocate 4096 bytes", MlxErrorCode::Inference),
            MlxErrorCode::AllocationFailure
        );
        assert_eq!(
            classify_error_message("unsupported operator scatter", MlxErrorCode::ModelLoad),
            MlxErrorCode::OperatorIncompatible
        );
        assert_eq!(
            classify_error_message("native process vanished", MlxErrorCode::NativeProcessExit),
            MlxErrorCode::NativeProcessExit
        );
        assert_eq!(
            classify_error_message("OOM while creating tensor", MlxErrorCode::NativeProcessExit),
            MlxErrorCode::AllocationFailure
        );
        assert_eq!(
            classify_error_message("zoom level failed", MlxErrorCode::NativeProcessExit),
            MlxErrorCode::NativeProcessExit
        );
    }

    #[test]
    fn worker_rejects_transcribe_before_handshake() {
        let dir = test_dir();
        let (mut client, server) = UnixStream::pair().unwrap();
        let worker_dir = dir.clone();
        let worker = thread::spawn(move || serve_worker_stream(server, &worker_dir).unwrap());
        write_frame(
            &mut client,
            &WorkerRequest::Transcribe {
                request_id: 1,
                wav_path: "missing.wav".to_string(),
            },
        )
        .unwrap();
        let response = read_frame::<WorkerResponse>(&mut client).unwrap().unwrap();
        assert!(matches!(
            response,
            WorkerResponse::Error {
                code: MlxErrorCode::ProtocolMismatch,
                ..
            }
        ));
        drop(client);
        worker.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn worker_rejects_unknown_protocol_version() {
        let dir = test_dir();
        let (mut client, server) = UnixStream::pair().unwrap();
        let worker_dir = dir.clone();
        let worker = thread::spawn(move || serve_worker_stream(server, &worker_dir).unwrap());
        write_frame(
            &mut client,
            &WorkerRequest::Hello {
                request_id: 1,
                protocol_version: PROTOCOL_VERSION + 1,
            },
        )
        .unwrap();
        let response = read_frame::<WorkerResponse>(&mut client).unwrap().unwrap();
        assert!(matches!(
            response,
            WorkerResponse::Error {
                request_id: 1,
                phase: WorkerPhase::Handshake,
                code: MlxErrorCode::ProtocolMismatch,
                ..
            }
        ));
        worker.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn model_validation_failure_is_structured_without_initializing_mlx() {
        let dir = test_dir();
        let missing_model = dir.join("missing-model");
        let (mut client, server) = UnixStream::pair().unwrap();
        let worker_dir = dir.clone();
        let worker = thread::spawn(move || serve_worker_stream(server, &worker_dir).unwrap());
        write_frame(
            &mut client,
            &WorkerRequest::Hello {
                request_id: 1,
                protocol_version: PROTOCOL_VERSION,
            },
        )
        .unwrap();
        assert!(matches!(
            read_frame::<WorkerResponse>(&mut client).unwrap(),
            Some(WorkerResponse::HelloAck { request_id: 1, .. })
        ));
        write_frame(
            &mut client,
            &WorkerRequest::Load {
                request_id: 2,
                model_dir: missing_model.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        assert!(matches!(
            read_frame::<WorkerResponse>(&mut client).unwrap(),
            Some(WorkerResponse::Progress {
                request_id: 2,
                phase: WorkerPhase::ModelValidation
            })
        ));
        assert!(matches!(
            read_frame::<WorkerResponse>(&mut client).unwrap(),
            Some(WorkerResponse::Error {
                request_id: 2,
                phase: WorkerPhase::ModelValidation,
                code: MlxErrorCode::ModelValidation,
                ..
            })
        ));
        drop(client);
        worker.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fake_worker_transcribes_and_temporary_wav_is_removed() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let child = sleeping_child();
        let client = test_client(client_stream, dir.clone(), child);
        let (path_tx, path_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let request = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let WorkerRequest::Transcribe {
                request_id,
                wav_path,
            } = request
            else {
                panic!("expected transcribe request");
            };
            assert!(Path::new(&wav_path).is_file());
            assert_eq!(
                fs::metadata(&wav_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            path_tx.send(wav_path).unwrap();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Progress {
                    request_id,
                    phase: WorkerPhase::Inference,
                },
            )
            .unwrap();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Transcript {
                    request_id,
                    text: "hello".to_string(),
                },
            )
            .unwrap();
        });
        assert_eq!(client.transcribe_pcm(&[0.0, 0.25]).unwrap(), "hello");
        let wav_path = path_rx.recv().unwrap();
        assert!(!Path::new(&wav_path).exists());
        worker.join().unwrap();
        drop(client);
    }

    #[test]
    fn one_worker_handles_handshake_load_and_multiple_transcribes_serially() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let client = test_client(client_stream, dir, sleeping_child());
        let worker = thread::spawn(move || {
            let hello = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let hello_id = hello.request_id();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::HelloAck {
                    request_id: hello_id,
                    protocol_version: PROTOCOL_VERSION,
                    worker_pid: 123,
                },
            )
            .unwrap();

            let load = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let load_id = load.request_id();
            assert!(matches!(load, WorkerRequest::Load { .. }));
            for phase in [
                WorkerPhase::ModelValidation,
                WorkerPhase::MlxInitialization,
                WorkerPhase::ModelLoad,
            ] {
                write_frame(
                    &mut worker_stream,
                    &WorkerResponse::Progress {
                        request_id: load_id,
                        phase,
                    },
                )
                .unwrap();
            }
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Loaded {
                    request_id: load_id,
                },
            )
            .unwrap();

            for expected in ["first", "second"] {
                let transcribe = read_frame::<WorkerRequest>(&mut worker_stream)
                    .unwrap()
                    .unwrap();
                let transcribe_id = transcribe.request_id();
                assert!(matches!(transcribe, WorkerRequest::Transcribe { .. }));
                write_frame(
                    &mut worker_stream,
                    &WorkerResponse::Progress {
                        request_id: transcribe_id,
                        phase: WorkerPhase::Inference,
                    },
                )
                .unwrap();
                write_frame(
                    &mut worker_stream,
                    &WorkerResponse::Transcript {
                        request_id: transcribe_id,
                        text: expected.to_string(),
                    },
                )
                .unwrap();
            }
        });

        client.handshake(Duration::from_secs(1)).unwrap();
        client.load_model(Path::new("/tmp/model")).unwrap();
        assert_eq!(client.transcribe_pcm(&[0.0]).unwrap(), "first");
        assert_eq!(client.transcribe_pcm(&[0.0]).unwrap(), "second");
        assert!(client.is_healthy());
        worker.join().unwrap();
        client.abort();
    }

    #[test]
    fn structured_allocation_error_marks_worker_unhealthy_and_stays_concise() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let client = test_client(client_stream, dir, sleeping_child());
        let (path_tx, path_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let request = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let WorkerRequest::Transcribe {
                request_id,
                wav_path,
            } = request
            else {
                panic!("expected transcribe request");
            };
            path_tx.send(wav_path).unwrap();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Progress {
                    request_id,
                    phase: WorkerPhase::Inference,
                },
            )
            .unwrap();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Error {
                    request_id,
                    phase: WorkerPhase::Inference,
                    code: MlxErrorCode::Inference,
                    message: "failed to allocate 4096 bytes".to_string(),
                },
            )
            .unwrap();
        });
        let error = client.transcribe_pcm(&[0.0]).unwrap_err().to_string();
        assert_eq!(error, "MLX/Metal 内存分配失败");
        assert!(!Path::new(&path_rx.recv().unwrap()).exists());
        assert!(!client.is_healthy());
        worker.join().unwrap();
    }

    #[test]
    fn worker_exit_during_load_reports_last_progress_phase() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let client = test_client(client_stream, dir, sleeping_child());
        let worker = thread::spawn(move || {
            let request = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let request_id = request.request_id();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Progress {
                    request_id,
                    phase: WorkerPhase::ModelLoad,
                },
            )
            .unwrap();
        });
        let error = client.load_model(Path::new("/tmp/model")).unwrap_err();
        assert_eq!(error.to_string(), "MLX worker 异常退出");
        assert_eq!(client.phase(), WorkerPhase::ModelLoad);
        assert!(!client.is_healthy());
        worker.join().unwrap();
    }

    #[test]
    fn worker_exit_during_inference_reports_last_progress_phase_and_cleans_wav() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let client = test_client(client_stream, dir, sleeping_child());
        let (path_tx, path_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let request = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let WorkerRequest::Transcribe {
                request_id,
                wav_path,
            } = request
            else {
                panic!("expected transcribe request");
            };
            path_tx.send(wav_path).unwrap();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Progress {
                    request_id,
                    phase: WorkerPhase::Inference,
                },
            )
            .unwrap();
        });
        let error = client.transcribe_pcm(&[0.0]).unwrap_err();
        assert_eq!(error.to_string(), "MLX worker 异常退出");
        assert_eq!(client.phase(), WorkerPhase::Inference);
        assert!(!Path::new(&path_rx.recv().unwrap()).exists());
        assert!(!client.is_healthy());
        worker.join().unwrap();
    }

    #[test]
    fn cancelling_non_owner_does_not_abort_active_operation_and_busy_is_explicit() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let client = Arc::new(test_client(client_stream, dir, sleeping_child()));
        let active_operation = client.next_operation_id();
        let other_operation = client.next_operation_id();
        let active_cancelled = Arc::new(AtomicBool::new(false));
        let other_cancelled = AtomicBool::new(false);
        let (request_tx, request_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let request = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let WorkerRequest::Transcribe { request_id, .. } = request else {
                panic!("expected transcribe request");
            };
            request_tx.send(()).unwrap();
            finish_rx.recv().unwrap();
            write_frame(
                &mut worker_stream,
                &WorkerResponse::Transcript {
                    request_id,
                    text: "active".to_string(),
                },
            )
            .unwrap();
        });
        let transcribing = {
            let client = Arc::clone(&client);
            let cancelled = Arc::clone(&active_cancelled);
            thread::spawn(move || {
                client.transcribe_pcm_for_operation(active_operation, &[0.0], &cancelled)
            })
        };
        request_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let error = client
            .transcribe_pcm_for_operation(other_operation, &[0.0], &other_cancelled)
            .unwrap_err();
        assert_eq!(error.to_string(), "MLX worker 正在处理另一个转写请求");
        other_cancelled.store(true, Ordering::Release);
        client.cancel_operation(other_operation);
        assert!(client.is_healthy());

        finish_tx.send(()).unwrap();
        assert_eq!(transcribing.join().unwrap().unwrap(), "active");
        worker.join().unwrap();
        client.abort();
    }

    #[test]
    fn cancelling_before_registration_prevents_the_operation_without_aborting_worker() {
        let dir = test_dir();
        let (client_stream, _worker_stream) = UnixStream::pair().unwrap();
        let client = test_client(client_stream, dir, sleeping_child());
        let operation_id = client.next_operation_id();
        let cancelled = AtomicBool::new(true);

        client.cancel_operation(operation_id);
        let error = client
            .transcribe_pcm_for_operation(operation_id, &[0.0], &cancelled)
            .unwrap_err();

        assert_eq!(error.to_string(), "MLX worker 转写已取消");
        assert!(client.is_healthy());
        client.abort();
    }

    #[test]
    fn cancelling_owner_unblocks_its_inflight_request() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let client = Arc::new(test_client(client_stream, dir, sleeping_child()));
        let operation_id = client.next_operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let (request_tx, request_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let request = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            assert!(matches!(request, WorkerRequest::Transcribe { .. }));
            request_tx.send(()).unwrap();
            thread::sleep(Duration::from_secs(1));
        });
        let transcribing = {
            let client = Arc::clone(&client);
            let cancelled = Arc::clone(&cancelled);
            thread::spawn(move || {
                client.transcribe_pcm_for_operation(operation_id, &[0.0], &cancelled)
            })
        };
        request_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        cancelled.store(true, Ordering::Release);
        let started = Instant::now();
        client.cancel_operation(operation_id);

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(transcribing.join().unwrap().is_err());
        assert!(!client.is_healthy());
        worker.join().unwrap();
    }

    #[test]
    fn abort_unblocks_an_inflight_request_without_waiting_for_the_io_lock() {
        let dir = test_dir();
        let (client_stream, mut worker_stream) = UnixStream::pair().unwrap();
        let client = Arc::new(test_client(client_stream, dir, sleeping_child()));
        let (request_tx, request_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let request = read_frame::<WorkerRequest>(&mut worker_stream)
                .unwrap()
                .unwrap();
            let WorkerRequest::Transcribe { wav_path, .. } = request else {
                panic!("expected transcribe request");
            };
            request_tx.send(wav_path).unwrap();
            thread::sleep(Duration::from_secs(1));
        });
        let transcribing = {
            let client = Arc::clone(&client);
            thread::spawn(move || client.transcribe_pcm(&[0.0]))
        };
        let wav_path = request_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let started = Instant::now();
        client.abort();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(transcribing.join().unwrap().is_err());
        assert!(!Path::new(&wav_path).exists());
        worker.join().unwrap();
    }

    #[test]
    fn abort_terminates_a_hung_worker_without_io_lock() {
        let dir = test_dir();
        let (client_stream, _worker_stream) = UnixStream::pair().unwrap();
        let client = test_client(client_stream, dir, sleeping_child());
        client.abort();
        assert!(!client.is_healthy());
        assert!(client.child.lock().unwrap().is_none());
    }

    #[test]
    fn health_check_detects_an_idle_worker_exit() {
        let dir = test_dir();
        let (client_stream, _worker_stream) = UnixStream::pair().unwrap();
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 17"])
            .spawn()
            .unwrap();
        let client = test_client(client_stream, dir, child);
        let deadline = Instant::now() + Duration::from_secs(1);
        while client.is_healthy() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!client.is_healthy());
    }

    #[test]
    fn startup_exit_is_detected_before_the_handshake_timeout() {
        let dir = test_dir();
        let socket_path = dir.join("worker.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 23"])
            .spawn()
            .unwrap();
        let started = Instant::now();
        let result = accept_worker(&listener, &mut child, started, &Diagnostics::default());
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn capture_threads_drain_large_stdout_and_stderr_without_blocking() {
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                "head -c 131072 /dev/zero | tr '\\0' o; head -c 131072 /dev/zero | tr '\\0' e >&2",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let diagnostics = Diagnostics::default();
        let stdout = spawn_capture(child.stdout.take().unwrap(), diagnostics.stdout.clone());
        let stderr = spawn_capture(child.stderr.take().unwrap(), diagnostics.stderr.clone());
        assert!(child.wait().unwrap().success());
        stdout.join().unwrap();
        stderr.join().unwrap();
        assert_eq!(diagnostics.stdout.text().len(), DIAGNOSTIC_TAIL_BYTES);
        assert_eq!(diagnostics.stderr.text().len(), DIAGNOSTIC_TAIL_BYTES);
        assert!(diagnostics.stdout.text().bytes().all(|byte| byte == b'o'));
        assert!(diagnostics.stderr.text().bytes().all(|byte| byte == b'e'));
    }

    #[test]
    fn graceful_shutdown_forces_a_worker_that_ignores_shutdown() {
        let dir = test_dir();
        let (client_stream, _worker_stream) = UnixStream::pair().unwrap();
        let client = test_client(client_stream, dir, sleeping_child());
        let started = Instant::now();
        client.graceful_shutdown();
        assert!(started.elapsed() >= SHUTDOWN_TIMEOUT);
        assert!(started.elapsed() < SHUTDOWN_TIMEOUT + Duration::from_secs(1));
        assert!(client.child.lock().unwrap().is_none());
    }

    #[test]
    fn bounded_diagnostic_tail_keeps_only_the_end() {
        let tail = TailBuffer::default();
        tail.append(&vec![b'a'; DIAGNOSTIC_TAIL_BYTES]);
        tail.append(b"the-end");
        let text = tail.text();
        assert_eq!(text.len(), DIAGNOSTIC_TAIL_BYTES);
        assert!(text.ends_with("the-end"));
    }
}
