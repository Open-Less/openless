//! Optional Python/MLX environment. Installation is explicit and never imports
//! inference libraries. Nothing is installed globally or discovered on PATH.
use openless_core::{BackendError, BackendErrorCode};
use sha2::{Digest, Sha256};
use std::{
    os::{
        fd::AsRawFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::AsyncReadExt;

pub const WORKER: &str = include_str!("worker.py");
const REQUIREMENTS: &str = include_str!("requirements.lock");
const GUARDIAN: &str = include_str!("install-guardian.sh");
const LICENSE: &str = include_str!("LICENSE-TextStream");
const PYTHON: &str = "3.12.15";
const UV_URL: &str = "https://files.pythonhosted.org/packages/3b/31/fad7fd2ac81954d02d33765bf8535eb320fcc2d71f2603a5c1ba0b13b8ef/uv-0.13.1-py3-none-macosx_11_0_arm64.whl";
const UV_BYTES: usize = 14391237;
const UV_SHA256: &str = "cfa719fbbd23f26713394dea2692b7f146aa1b5713ce5e776e878701214ea52c";

fn error(message: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, message.to_string())
}
fn marker(directory: &Path) -> String {
    format!(
        "{}:{}:{}:{}",
        PYTHON,
        UV_SHA256,
        directory.display(),
        format_args!("{:x}", Sha256::digest(format!("{WORKER}\n{REQUIREMENTS}")))
    )
}
pub fn python(directory: &Path) -> PathBuf {
    directory.join(".runtime/venv/bin/python")
}
pub fn script(directory: &Path) -> PathBuf {
    directory.join(".runtime/worker.py")
}
fn current_worker_installed(directory: &Path) -> bool {
    python(directory).is_file()
        && std::fs::read_to_string(directory.join(".runtime/installed"))
            .is_ok_and(|s| s == marker(directory))
        && std::fs::read_to_string(script(directory)).is_ok_and(|s| s == WORKER)
}

const PREVIOUS_WORKERS: &[(&str, &str)] = &[
    (
        "f585f75ca2d669b48f75a40fe6a7213f9948ac84ce3c5c9378f2b84dfe77a243",
        "6c6e1ce76c0e3e220b35160744c5253b499d364095e294c45d712ed7b0922184",
    ),
    (
        "64e7ec7b3c3557bea2314d8a99564cf7cfb0402ff1cea586fc3c5366ef5e543b",
        "f08166fc2a375a42f9c86bcc75ded43e1bc8b742c4fba4289db4002a6bc464dc",
    ),
];

/// A known managed worker can be upgraded at recording start without reinstalling
/// Python. Lease/status must recognize it, otherwise start never reaches refresh.
pub fn installed(directory: &Path) -> bool {
    if current_worker_installed(directory) {
        return true;
    }
    let root = directory.join(".runtime");
    if !python(directory).is_file()
        || !std::fs::read_to_string(root.join("requirements.lock")).is_ok_and(|s| s == REQUIREMENTS)
    {
        return false;
    }
    let Ok(receipt) = std::fs::read_to_string(root.join("installed")) else {
        return false;
    };
    let Ok(worker) = std::fs::read(script(directory)) else {
        return false;
    };
    let hash = format!("{:x}", Sha256::digest(&worker));
    PREVIOUS_WORKERS.iter().any(|(environment, expected)| {
        receipt == format!("{PYTHON}:{UV_SHA256}:{}:{environment}", directory.display())
            && (hash == *expected || worker == WORKER.as_bytes())
    })
}

struct CommandGuard {
    child: tokio::process::Child,
}
impl Drop for CommandGuard {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            if let Some(pid) = self.child.id() {
                // This command created its own process group; never target a
                // global service or somebody else's Python process.
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = self.child.start_kill();
        }
    }
}

async fn run(
    uv: &Path,
    runtime: &Path,
    lock: &std::fs::File,
    args: &[&std::ffi::OsStr],
) -> Result<(), BackendError> {
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .arg(runtime.join("install-guardian.sh"))
        .arg(std::process::id().to_string())
        .arg(uv)
        .args(args)
        .arg("--no-config")
        .current_dir(runtime)
        .env("UV_PYTHON_INSTALL_DIR", runtime.join("python"))
        .env("UV_CACHE_DIR", runtime.join("cache"))
        .env("UV_PYTHON_DOWNLOADS", "automatic")
        .env("PYTHONNOUSERSITE", "1")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .env_remove("UV_INDEX_URL")
        .env_remove("UV_EXTRA_INDEX_URL")
        .env_remove("UV_DEFAULT_INDEX")
        .env_remove("UV_INDEX")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.as_std_mut().process_group(0);
    let lock_fd = lock.as_raw_fd();
    // The guardian inherits the lock, so restart cannot race a still-exiting installer.
    // Only fcntl is used between fork/exec; it is async-signal-safe.
    unsafe {
        command.as_std_mut().pre_exec(move || {
            if libc::fcntl(lock_fd, libc::F_SETFD, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut guard = CommandGuard {
        child: command.spawn().map_err(error)?,
    };
    let mut stderr = guard.child.stderr.take().expect("piped stderr");
    let diagnostics = tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut buffer = [0; 4096];
        while let Ok(n) = stderr.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&buffer[..n]);
            if tail.len() > 16_384 {
                tail.drain(..tail.len() - 16_384);
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    });
    let status = tokio::time::timeout(std::time::Duration::from_secs(900), guard.child.wait())
        .await
        .map_err(|_| error("TextStream runtime installation timed out; retry explicitly"))?
        .map_err(error)?;
    let diagnostics = diagnostics.await.unwrap_or_default();
    if !status.success() {
        return Err(error(format!(
            "TextStream runtime installation failed ({status}): {diagnostics}"
        )));
    }
    Ok(())
}

async fn installer_bytes(path: &Path) -> Result<Vec<u8>, BackendError> {
    if let Ok(bytes) = std::fs::read(path) {
        if bytes.len() == UV_BYTES && format!("{:x}", Sha256::digest(&bytes)) == UV_SHA256 {
            return Ok(bytes);
        }
    }
    use openless_core::ModelTransport;
    let transport = openless_core::ReqwestModelTransport::new()?;
    let partial = path.with_extension("part");
    let mut bytes = std::fs::read(&partial).unwrap_or_default();
    if bytes.len() > UV_BYTES {
        bytes.clear();
    }
    while bytes.len() < UV_BYTES {
        let start = bytes.len() as u64;
        let end = (start + 1024 * 1024 - 1).min(UV_BYTES as u64 - 1);
        let response = transport
            .request(openless_core::ModelTransportRequest {
                url: UV_URL.into(),
                range: Some((start, end)),
                max_response_bytes: UV_BYTES as u64,
            })
            .await?;
        if response.status == 200 && response.bytes.len() == UV_BYTES {
            bytes = response.bytes;
        } else if response.status == 206
            && response.metadata.content_range
                == Some(openless_core::ModelContentRange {
                    start,
                    end,
                    total: UV_BYTES as u64,
                })
            && response.bytes.len() as u64 == end - start + 1
        {
            bytes.extend_from_slice(&response.bytes);
        } else {
            return Err(error("Invalid TextStream installer range response"));
        }
        std::fs::write(&partial, &bytes).map_err(error)?;
    }
    if bytes.len() != UV_BYTES || format!("{:x}", Sha256::digest(&bytes)) != UV_SHA256 {
        let _ = std::fs::remove_file(&partial);
        return Err(error("TextStream installer checksum mismatch"));
    }
    std::fs::rename(&partial, path).map_err(error)?;
    Ok(bytes)
}

async fn installation_lock(root: &Path) -> Result<std::fs::File, BackendError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join("install.lock"))
        .map_err(error)?;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(());
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::WouldBlock {
                return Err(error(e));
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| error("Another TextStream installation is still running; retry later"))??;
    Ok(file)
}

/// Upgrade only our worker when the pinned Python/dependency environment is
/// unchanged. Never redownload weights or clear a working venv for a script fix.
pub async fn refresh_worker(directory: &Path) -> Result<bool, BackendError> {
    if current_worker_installed(directory) {
        return Ok(true);
    }
    let root = directory.join(".runtime");
    if !std::fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return Ok(false);
    }
    let _lock = installation_lock(&root).await?;
    refresh_worker_locked(directory)
}

fn refresh_worker_locked(directory: &Path) -> Result<bool, BackendError> {
    if current_worker_installed(directory) {
        return Ok(true);
    }
    if !installed(directory) {
        return Ok(false);
    }
    let root = directory.join(".runtime");
    fn replace(path: &Path, bytes: &[u8]) -> Result<(), BackendError> {
        use std::io::Write;
        let temporary = path.with_extension(format!("{}.update", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)
                .map_err(error)?;
            file.write_all(bytes).map_err(error)?;
            file.sync_all().map_err(error)?;
            std::fs::rename(&temporary, path).map_err(error)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
    replace(&script(directory), WORKER.as_bytes())?;
    replace(&root.join("installed"), marker(directory).as_bytes())?;
    Ok(installed(directory))
}

#[cfg(test)]
mod worker_update_tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[tokio::test]
    async fn worker_upgrade_preserves_the_pinned_environment_and_rejects_unknown_receipts() {
        let fixture = Fixture(
            std::env::temp_dir().join(format!("openless-worker-upgrade-{}", uuid::Uuid::new_v4())),
        );
        let directory = &fixture.0;
        std::fs::create_dir_all(python(directory).parent().unwrap()).unwrap();
        std::fs::write(
            python(directory),
            b"existing managed Python, never execute in test",
        )
        .unwrap();
        let root = directory.join(".runtime");
        std::fs::write(root.join("requirements.lock"), REQUIREMENTS).unwrap();
        std::fs::write(script(directory), WORKER).unwrap();
        for (environment, _) in PREVIOUS_WORKERS {
            let receipt = format!("{PYTHON}:{UV_SHA256}:{}:{environment}", directory.display());
            std::fs::write(root.join("installed"), receipt).unwrap();
            assert!(
                installed(directory),
                "lease must allow a known worker to reach refresh"
            );
            assert!(!current_worker_installed(directory));
            assert!(refresh_worker(directory).await.unwrap());
            assert!(current_worker_installed(directory));
        }
        let previous = format!("{PYTHON}:{UV_SHA256}:{}:f585f75ca2d669b48f75a40fe6a7213f9948ac84ce3c5c9378f2b84dfe77a243", directory.display());
        std::fs::write(root.join("installed"), &previous).unwrap();
        assert!(
            refresh_worker(directory).await.unwrap(),
            "recover script/receipt upgrade without reinstalling Python"
        );
        assert!(installed(directory));
        assert_eq!(
            std::fs::read(python(directory)).unwrap(),
            b"existing managed Python, never execute in test"
        );
        std::fs::write(root.join("installed"), "unknown environment").unwrap();
        assert!(!refresh_worker(directory).await.unwrap());
        assert_eq!(
            std::fs::read_to_string(root.join("installed")).unwrap(),
            "unknown environment"
        );
        std::fs::write(root.join("installed"), previous).unwrap();
        std::fs::write(script(directory), "user-modified worker").unwrap();
        assert!(
            !refresh_worker(directory).await.unwrap(),
            "never overwrite an unknown worker"
        );
        assert_eq!(
            std::fs::read_to_string(script(directory)).unwrap(),
            "user-modified worker"
        );
    }
}

pub async fn install(directory: PathBuf) -> Result<(), BackendError> {
    std::fs::create_dir_all(&directory).map_err(error)?;
    std::fs::write(directory.join("LICENSE-TextStream"), LICENSE).map_err(error)?;
    if refresh_worker(&directory).await? {
        return Ok(());
    }
    let root = directory.join(".runtime");
    std::fs::create_dir_all(&root).map_err(error)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).map_err(error)?;
    let lock_guard = installation_lock(&root).await?;
    if installed(&directory) {
        return Ok(());
    }
    std::fs::write(root.join("install-guardian.sh"), GUARDIAN).map_err(error)?;
    let _ = std::fs::remove_file(root.join("installed"));
    let uv = root.join("uv");
    let archive = root.join("uv.whl");
    let bytes = installer_bytes(&archive).await?;
    let destination = uv.clone();
    // Executable downloads are pinned and verified BEFORE extraction/execution.
    tokio::task::spawn_blocking(move || {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(error)?;
        let mut entry = zip.by_name("uv-0.13.1.data/scripts/uv").map_err(error)?;
        let mut output = std::fs::File::create(&destination).map_err(error)?;
        std::io::copy(&mut entry, &mut output).map_err(error)?;
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o700))
            .map_err(error)?;
        Ok::<(), BackendError>(())
    })
    .await
    .map_err(error)??;
    std::fs::write(script(&directory), WORKER).map_err(error)?;
    let lock = root.join("requirements.lock");
    std::fs::write(&lock, REQUIREMENTS).map_err(error)?;
    let venv = root.join("venv");
    run(
        &uv,
        &root,
        &lock_guard,
        &[
            "venv".as_ref(),
            "--clear".as_ref(),
            "--python".as_ref(),
            PYTHON.as_ref(),
            "--managed-python".as_ref(),
            venv.as_os_str(),
        ],
    )
    .await?;
    let python = python(&directory);
    run(
        &uv,
        &root,
        &lock_guard,
        &[
            "pip".as_ref(),
            "sync".as_ref(),
            "--python".as_ref(),
            python.as_os_str(),
            "--require-hashes".as_ref(),
            "--only-binary".as_ref(),
            ":all:".as_ref(),
            "--index-url".as_ref(),
            "https://pypi.org/simple".as_ref(),
            lock.as_os_str(),
        ],
    )
    .await?;
    // The interpreter and wheels are now on disk. Do not probe by importing MLX.
    std::fs::write(root.join("installed.tmp"), marker(&directory)).map_err(error)?;
    std::fs::rename(root.join("installed.tmp"), root.join("installed")).map_err(error)?;
    Ok(())
}
