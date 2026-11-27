use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::thread::JoinHandle;
#[cfg(target_os = "linux")]
use std::time::Duration;

use fs2::FileExt;
#[cfg(any(target_os = "linux", test))]
use openless_core::LaunchIntent;
use openless_core::{parse_cli_intent, BackendError, BackendErrorCode, CliIntent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxLaunchIntent {
    ShowMain,
    Cli(CliIntent),
}

impl LinuxLaunchIntent {
    pub fn from_args<S: AsRef<str>>(args: &[S]) -> Self {
        parse_cli_intent(args).map_or(Self::ShowMain, Self::Cli)
    }
}

#[cfg(any(target_os = "linux", test))]
fn encode_launch_intent(intent: LinuxLaunchIntent) -> Vec<u8> {
    openless_core::encode_launch_intent(match intent {
        LinuxLaunchIntent::ShowMain => LaunchIntent::ShowMain,
        LinuxLaunchIntent::Cli(intent) => LaunchIntent::Cli { intent },
    })
}

#[cfg(any(target_os = "linux", test))]
fn decode_launch_intent(message: &[u8]) -> Option<LinuxLaunchIntent> {
    match openless_core::decode_launch_intent(message)? {
        LaunchIntent::ShowMain => Some(LinuxLaunchIntent::ShowMain),
        LaunchIntent::Cli { intent } => Some(LinuxLaunchIntent::Cli(intent)),
    }
}

/// Process-lifetime file lock used before the windowing runtime starts.
pub struct SingleInstanceGuard {
    file: File,
}

impl SingleInstanceGuard {
    pub fn acquire(path: &Path) -> Result<Option<Self>, BackendError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Persistence,
                    format!("failed to create single-instance directory: {error}"),
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Persistence,
                    format!("failed to open single-instance lock: {error}"),
                )
            })?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(33) =>
            {
                Ok(None)
            }
            Err(error) => Err(BackendError::new(
                BackendErrorCode::Platform,
                format!("failed to acquire single-instance lock: {error}"),
            )),
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

pub enum SingleInstanceRole {
    Primary(SingleInstanceBroker),
    Forwarded,
}

/// Linux single-instance Adapter with a private Unix socket for launcher intent
/// forwarding. The primary process drains typed intents from its UI/runtime
/// loop; secondary processes wait for an acknowledgement before exiting.
pub struct SingleInstanceBroker {
    _guard: SingleInstanceGuard,
    intents: Arc<Mutex<VecDeque<LinuxLaunchIntent>>>,
    last_error: Arc<Mutex<Option<String>>>,
    #[cfg(target_os = "linux")]
    socket_path: PathBuf,
    #[cfg(target_os = "linux")]
    shutdown: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    worker: Option<JoinHandle<()>>,
}

impl SingleInstanceBroker {
    pub fn acquire_or_forward(
        lock_path: &Path,
        socket_path: &Path,
        intent: LinuxLaunchIntent,
    ) -> Result<SingleInstanceRole, BackendError> {
        #[cfg(target_os = "linux")]
        {
            if let Some(guard) = SingleInstanceGuard::acquire(lock_path)? {
                return Self::bind_primary(guard, socket_path).map(SingleInstanceRole::Primary);
            }
            forward_to_primary(socket_path, intent)?;
            Ok(SingleInstanceRole::Forwarded)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (lock_path, socket_path, intent);
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "Linux single-instance intent forwarding is unavailable on this target",
            ))
        }
    }

    pub fn drain(&self, mut apply: impl FnMut(LinuxLaunchIntent)) -> usize {
        let intents = {
            let mut pending = self.intents.lock().expect("launch intent queue poisoned");
            pending.drain(..).collect::<Vec<_>>()
        };
        let count = intents.len();
        for intent in intents {
            apply(intent);
        }
        count
    }

    pub fn take_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .expect("single-instance error lock poisoned")
            .take()
    }

    #[cfg(target_os = "linux")]
    fn bind_primary(guard: SingleInstanceGuard, socket_path: &Path) -> Result<Self, BackendError> {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        use std::os::unix::net::UnixListener;

        let parent = socket_path.parent().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidArgument,
                "single-instance socket path has no parent directory",
            )
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            BackendError::new(
                BackendErrorCode::Persistence,
                format!("failed to create single-instance socket directory: {error}"),
            )
        })?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                BackendError::new(
                    BackendErrorCode::Persistence,
                    format!("failed to protect single-instance socket directory: {error}"),
                )
            },
        )?;
        match std::fs::symlink_metadata(socket_path) {
            Ok(metadata) if metadata.file_type().is_socket() => {
                std::fs::remove_file(socket_path).map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Platform,
                        format!("failed to remove stale single-instance socket: {error}"),
                    )
                })?;
            }
            Ok(_) => {
                return Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "single-instance socket path exists and is not a Unix socket",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(BackendError::new(
                    BackendErrorCode::Platform,
                    format!("failed to inspect single-instance socket: {error}"),
                ));
            }
        }

        let listener = UnixListener::bind(socket_path).map_err(|error| {
            BackendError::new(
                BackendErrorCode::Platform,
                format!("failed to bind single-instance socket: {error}"),
            )
        })?;
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("failed to protect single-instance socket: {error}"),
                )
            },
        )?;
        listener.set_nonblocking(true).map_err(|error| {
            BackendError::new(
                BackendErrorCode::Platform,
                format!("failed to configure single-instance socket: {error}"),
            )
        })?;

        let intents = Arc::new(Mutex::new(VecDeque::new()));
        let last_error = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_intents = Arc::clone(&intents);
        let worker_error = Arc::clone(&last_error);
        let worker_shutdown = Arc::clone(&shutdown);
        let worker = std::thread::Builder::new()
            .name("openless-single-instance".into())
            .spawn(move || {
                while !worker_shutdown.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            if let Err(error) = receive_intent(&mut stream, &worker_intents) {
                                *worker_error
                                    .lock()
                                    .expect("single-instance error lock poisoned") =
                                    Some(error.to_string());
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        Err(error) => {
                            *worker_error
                                .lock()
                                .expect("single-instance error lock poisoned") =
                                Some(format!("single-instance listener failed: {error}"));
                            break;
                        }
                    }
                }
            })
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("failed to start single-instance listener: {error}"),
                )
            })?;

        Ok(Self {
            _guard: guard,
            intents,
            last_error,
            socket_path: socket_path.to_path_buf(),
            shutdown,
            worker: Some(worker),
        })
    }
}

#[cfg(target_os = "linux")]
fn receive_intent(
    stream: &mut std::os::unix::net::UnixStream,
    intents: &Mutex<VecDeque<LinuxLaunchIntent>>,
) -> Result<(), BackendError> {
    const MAX_MESSAGE_BYTES: usize = 64;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(single_instance_io_error)?;
    let mut buffer = [0_u8; MAX_MESSAGE_BYTES + 1];
    let count = stream.read(&mut buffer).map_err(single_instance_io_error)?;
    let Some(intent) = decode_launch_intent(&buffer[..count]) else {
        let _ = stream.write_all(b"invalid\n");
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "secondary instance sent an invalid launch intent",
        ));
    };
    intents
        .lock()
        .expect("launch intent queue poisoned")
        .push_back(intent);
    stream.write_all(b"ok\n").map_err(single_instance_io_error)
}

#[cfg(target_os = "linux")]
fn forward_to_primary(socket_path: &Path, intent: LinuxLaunchIntent) -> Result<(), BackendError> {
    use std::os::unix::net::UnixStream;

    let mut last_error = None;
    for _ in 0..40 {
        match UnixStream::connect(socket_path) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .map_err(single_instance_io_error)?;
                stream
                    .write_all(&encode_launch_intent(intent))
                    .map_err(single_instance_io_error)?;
                stream
                    .shutdown(std::net::Shutdown::Write)
                    .map_err(single_instance_io_error)?;
                let mut acknowledgement = [0_u8; 3];
                let count = stream
                    .read(&mut acknowledgement)
                    .map_err(single_instance_io_error)?;
                if &acknowledgement[..count] == b"ok\n" {
                    return Ok(());
                }
                return Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "primary instance rejected the launch intent",
                ));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(single_instance_io_error(error)),
        }
    }
    Err(single_instance_io_error(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "primary instance socket did not become ready",
        )
    })))
}

#[cfg(target_os = "linux")]
fn single_instance_io_error(error: std::io::Error) -> BackendError {
    BackendError::new(
        BackendErrorCode::Platform,
        format!("single-instance communication failed: {error}"),
    )
}

#[cfg(target_os = "linux")]
impl Drop for SingleInstanceBroker {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = std::os::unix::net::UnixStream::connect(&self.socket_path);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_intent_protocol_round_trips_every_supported_action() {
        let cases = [
            LinuxLaunchIntent::ShowMain,
            LinuxLaunchIntent::Cli(CliIntent::ToggleDictation),
            LinuxLaunchIntent::Cli(CliIntent::ToggleQa),
            LinuxLaunchIntent::Cli(CliIntent::CancelDictation),
        ];
        for intent in cases {
            assert_eq!(
                decode_launch_intent(&encode_launch_intent(intent)),
                Some(intent)
            );
        }
        assert_eq!(decode_launch_intent(b"unknown\n"), None);
    }

    #[test]
    fn launcher_args_default_to_show_main_and_preserve_core_cli_intents() {
        assert_eq!(
            LinuxLaunchIntent::from_args(&["openless"]),
            LinuxLaunchIntent::ShowMain
        );
        assert_eq!(
            LinuxLaunchIntent::from_args(&["openless", "--toggle-dictation"]),
            LinuxLaunchIntent::Cli(CliIntent::ToggleDictation)
        );
    }

    #[test]
    fn second_guard_is_rejected_until_the_first_is_dropped() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-single-instance-{}",
            std::process::id()
        ));
        let path = root.join("openless.lock");
        let first = SingleInstanceGuard::acquire(&path).unwrap().unwrap();
        assert!(SingleInstanceGuard::acquire(&path).unwrap().is_none());
        drop(first);
        assert!(SingleInstanceGuard::acquire(&path).unwrap().is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn secondary_instance_forwards_intent_to_primary_queue() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-intent-forwarding-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let lock = root.join("openless.lock");
        let socket = root.join("openless.sock");
        let primary = match SingleInstanceBroker::acquire_or_forward(
            &lock,
            &socket,
            LinuxLaunchIntent::ShowMain,
        )
        .unwrap()
        {
            SingleInstanceRole::Primary(primary) => primary,
            SingleInstanceRole::Forwarded => panic!("first instance must become primary"),
        };
        assert!(matches!(
            SingleInstanceBroker::acquire_or_forward(
                &lock,
                &socket,
                LinuxLaunchIntent::Cli(CliIntent::ToggleQa),
            )
            .unwrap(),
            SingleInstanceRole::Forwarded
        ));

        let mut received = Vec::new();
        for _ in 0..40 {
            primary.drain(|intent| received.push(intent));
            if !received.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(received, vec![LinuxLaunchIntent::Cli(CliIntent::ToggleQa)]);
        assert_eq!(primary.take_error(), None);
        drop(primary);
        let _ = std::fs::remove_dir_all(root);
    }
}
