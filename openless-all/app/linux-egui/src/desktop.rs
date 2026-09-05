//! Linux desktop integration that does not depend on Tauri.
//!
//! The functions in this module deliberately report failures instead of
//! treating a best-effort desktop operation as successful.  Slow operations
//! (D-Bus and process execution) are blocking and should be dispatched with
//! `tokio::task::spawn_blocking` by the UI bridge.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Duration;

const AUTOSTART_FILE: &str = "openless.desktop";
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum DesktopError {
    InvalidInput(String),
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Dbus(String),
    LauncherFailed {
        program: String,
        status: ExitStatus,
    },
    LauncherUnavailable(Vec<String>),
}

impl fmt::Display for DesktopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => f.write_str(message),
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::Dbus(message) => write!(f, "desktop notification D-Bus error: {message}"),
            Self::LauncherFailed { program, status } => {
                write!(f, "{program} exited unsuccessfully ({status})")
            }
            Self::LauncherUnavailable(errors) => {
                write!(
                    f,
                    "no desktop URL launcher was available: {}",
                    errors.join("; ")
                )
            }
        }
    }
}

impl std::error::Error for DesktopError {}

fn io_error(operation: &'static str, source: io::Error) -> DesktopError {
    DesktopError::Io { operation, source }
}

/// Manages the per-user XDG autostart entry for OpenLess.
#[derive(Debug, Clone)]
pub struct AutostartManager {
    entry_path: PathBuf,
    executable: PathBuf,
}

impl AutostartManager {
    pub fn detect(executable: PathBuf) -> Result<Self, DesktopError> {
        let config_home = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => std::env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
                .ok_or_else(|| {
                    DesktopError::InvalidInput(
                        "neither XDG_CONFIG_HOME nor HOME is available".into(),
                    )
                })?,
        };
        Self::new(
            config_home.join("autostart").join(AUTOSTART_FILE),
            executable,
        )
    }

    pub fn new(entry_path: PathBuf, executable: PathBuf) -> Result<Self, DesktopError> {
        validate_executable(&executable)?;
        if !entry_path.is_absolute() {
            return Err(DesktopError::InvalidInput(
                "autostart entry path must be absolute".into(),
            ));
        }
        Ok(Self {
            entry_path,
            executable,
        })
    }

    pub fn entry_path(&self) -> &Path {
        &self.entry_path
    }

    pub fn is_enabled(&self) -> Result<bool, DesktopError> {
        match fs::symlink_metadata(&self.entry_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(DesktopError::InvalidInput(
                "refusing to trust a symlinked autostart entry".into(),
            )),
            Ok(metadata) if metadata.is_file() => Ok(true),
            Ok(_) => Err(DesktopError::InvalidInput(
                "autostart entry exists but is not a regular file".into(),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_error("inspect autostart entry", error)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), DesktopError> {
        if enabled {
            // Inspect first so a hostile/pre-existing symlink is never silently
            // replaced and reported as a successfully managed entry.
            let _ = self.is_enabled()?;
            let contents = desktop_entry(&self.executable)?;
            atomic_write(&self.entry_path, contents.as_bytes(), Some(0o600))
        } else {
            match fs::symlink_metadata(&self.entry_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    Err(DesktopError::InvalidInput(
                        "refusing to remove a symlinked autostart entry".into(),
                    ))
                }
                Ok(metadata) if metadata.is_file() => {
                    fs::remove_file(&self.entry_path)
                        .map_err(|error| io_error("remove autostart entry", error))?;
                    sync_parent(&self.entry_path)
                }
                Ok(_) => Err(DesktopError::InvalidInput(
                    "autostart entry exists but is not a regular file".into(),
                )),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(io_error("inspect autostart entry", error)),
            }
        }
    }
}

fn validate_executable(executable: &Path) -> Result<(), DesktopError> {
    if !executable.is_absolute() {
        return Err(DesktopError::InvalidInput(
            "autostart executable must be absolute".into(),
        ));
    }
    let value = executable.as_os_str().to_string_lossy();
    if value.contains(['\n', '\r', '\0']) {
        return Err(DesktopError::InvalidInput(
            "autostart executable contains a forbidden control character".into(),
        ));
    }
    Ok(())
}

fn desktop_entry(executable: &Path) -> Result<String, DesktopError> {
    validate_executable(executable)?;
    let escaped = executable
        .as_os_str()
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    Ok(format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=OpenLess\nComment=Start OpenLess in the background\nExec=\"{escaped}\" --minimized\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    ))
}

/// A freedesktop.org desktop notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification<'a> {
    pub summary: &'a str,
    pub body: &'a str,
    pub icon: &'a str,
    /// Zero lets the notification server choose its default timeout.
    pub timeout_ms: i32,
}

/// Sends a notification and returns the notification server's identifier.
#[cfg(target_os = "linux")]
pub fn notify(request: Notification<'_>) -> Result<u32, DesktopError> {
    use dbus::arg::PropMap;
    use dbus::blocking::Connection;

    if request.summary.contains('\0') || request.body.contains('\0') {
        return Err(DesktopError::InvalidInput(
            "notification text contains a NUL byte".into(),
        ));
    }
    let connection =
        Connection::new_session().map_err(|error| DesktopError::Dbus(error.to_string()))?;
    let proxy = connection.with_proxy(
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        NOTIFICATION_TIMEOUT,
    );
    let hints: PropMap = std::collections::HashMap::new();
    let (id,): (u32,) = proxy
        .method_call(
            "org.freedesktop.Notifications",
            "Notify",
            (
                "OpenLess",
                0u32,
                request.icon,
                request.summary,
                request.body,
                Vec::<String>::new(),
                hints,
                request.timeout_ms,
            ),
        )
        .map_err(|error| DesktopError::Dbus(error.to_string()))?;
    Ok(id)
}

#[cfg(not(target_os = "linux"))]
pub fn notify(_request: Notification<'_>) -> Result<u32, DesktopError> {
    Err(DesktopError::InvalidInput(
        "desktop notifications are supported only on Linux".into(),
    ))
}

/// Opens an HTTP(S) URL using a desktop launcher and waits for the launcher to
/// acknowledge the request.  Returning `Ok` never means merely "spawned".
pub fn open_external(url: &str) -> Result<(), DesktopError> {
    validate_external_url(url)?;
    open_external_with(url, &[("xdg-open", &[]), ("gio", &["open"])])
}

fn validate_external_url(url: &str) -> Result<(), DesktopError> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(DesktopError::InvalidInput(
            "only HTTP(S) external URLs are allowed".into(),
        ));
    }
    if url.chars().any(char::is_control) {
        return Err(DesktopError::InvalidInput(
            "external URL contains a control character".into(),
        ));
    }
    Ok(())
}

fn open_external_with(url: &str, launchers: &[(&str, &[&str])]) -> Result<(), DesktopError> {
    let mut unavailable = Vec::new();
    for (program, prefix) in launchers {
        match Command::new(program).args(*prefix).arg(url).status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => {
                return Err(DesktopError::LauncherFailed {
                    program: (*program).into(),
                    status,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                unavailable.push(format!("{program}: {error}"));
            }
            Err(error) => return Err(io_error("start desktop URL launcher", error)),
        }
    }
    Err(DesktopError::LauncherUnavailable(unavailable))
}

/// Validates a user-selected destination for a safe atomic save.
pub fn validate_save_path(path: &Path) -> Result<PathBuf, DesktopError> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(DesktopError::InvalidInput(
            "save destination must be an absolute file path".into(),
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        DesktopError::InvalidInput("save destination has no parent directory".into())
    })?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| io_error("resolve save destination directory", error))?;
    if !canonical_parent.is_dir() {
        return Err(DesktopError::InvalidInput(
            "save destination parent is not a directory".into(),
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(DesktopError::InvalidInput(
            "refusing to overwrite a symlink".into(),
        )),
        Ok(metadata) if !metadata.is_file() => Err(DesktopError::InvalidInput(
            "save destination exists but is not a regular file".into(),
        )),
        Ok(_) => Ok(canonical_parent.join(path.file_name().expect("checked above"))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(canonical_parent.join(path.file_name().expect("checked above")))
        }
        Err(error) => Err(io_error("inspect save destination", error)),
    }
}

/// Atomically saves bytes without following an existing destination symlink.
pub fn atomic_save(path: &Path, bytes: &[u8]) -> Result<PathBuf, DesktopError> {
    let validated = validate_save_path(path)?;
    atomic_write(&validated, bytes, Some(0o600))?;
    Ok(validated)
}

fn atomic_write(path: &Path, bytes: &[u8], unix_mode: Option<u32>) -> Result<(), DesktopError> {
    let parent = path.parent().ok_or_else(|| {
        DesktopError::InvalidInput("atomic-write destination has no parent".into())
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error("create destination directory", error))?;
    let file_name = path.file_name().ok_or_else(|| {
        DesktopError::InvalidInput("atomic-write destination has no filename".into())
    })?;
    let temp = parent.join(format!(
        ".{}.tmp-{}",
        file_name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| io_error("create temporary file", error))?;
        #[cfg(unix)]
        if let Some(mode) = unix_mode {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(mode))
                .map_err(|error| io_error("set temporary file permissions", error))?;
        }
        file.write_all(bytes)
            .map_err(|error| io_error("write temporary file", error))?;
        file.sync_all()
            .map_err(|error| io_error("sync temporary file", error))?;
        fs::rename(&temp, path).map_err(|error| io_error("replace destination", error))?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn sync_parent(path: &Path) -> Result<(), DesktopError> {
    let parent = path
        .parent()
        .ok_or_else(|| DesktopError::InvalidInput("destination has no parent directory".into()))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync destination directory", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "openless-desktop-{name}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn autostart_is_atomic_and_round_trips() {
        let root = temp_dir("autostart");
        let entry = root.join("config/autostart/openless.desktop");
        let manager =
            AutostartManager::new(entry.clone(), PathBuf::from("/opt/Open Less/openless")).unwrap();
        assert!(!manager.is_enabled().unwrap());
        manager.set_enabled(true).unwrap();
        assert!(manager.is_enabled().unwrap());
        let text = fs::read_to_string(&entry).unwrap();
        assert!(text.contains("Exec=\"/opt/Open Less/openless\" --minimized"));
        assert_eq!(text.matches("[Desktop Entry]").count(), 1);
        manager.set_enabled(false).unwrap();
        assert!(!manager.is_enabled().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn autostart_refuses_to_replace_or_remove_symlinks() {
        use std::os::unix::fs::symlink;
        let root = temp_dir("autostart-symlink");
        let target = root.join("target");
        fs::write(&target, b"keep").unwrap();
        let entry = root.join("openless.desktop");
        symlink(&target, &entry).unwrap();
        let manager = AutostartManager::new(entry, PathBuf::from("/usr/bin/openless")).unwrap();
        assert!(manager.set_enabled(false).is_err());
        assert!(manager.set_enabled(true).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_path_is_canonical_and_atomic() {
        let root = temp_dir("save");
        let nested = root.join("nested");
        fs::create_dir(&nested).unwrap();
        let destination = nested.join("export.json");
        let saved = atomic_save(&destination, b"first").unwrap();
        assert!(saved.is_absolute());
        assert_eq!(fs::read(&destination).unwrap(), b"first");
        atomic_save(&destination, b"second").unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"second");
        assert_eq!(fs::read_dir(&nested).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_save_and_url_inputs_are_rejected() {
        assert!(validate_save_path(Path::new("relative.txt")).is_err());
        assert!(validate_external_url("file:///etc/passwd").is_err());
        assert!(validate_external_url("https://example.test/\nattack").is_err());
        assert!(validate_external_url("https://example.test/path").is_ok());
    }
}
