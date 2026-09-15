use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::time::Duration;

use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, InsertOutcome, InsertWriteResult, TextInserter,
    TextInsertionSession,
};

use crate::LinuxResourceLayout;

#[cfg(target_os = "linux")]
pub(crate) const DESTINATION: &str = "org.fcitx.Fcitx5";
#[cfg(target_os = "linux")]
pub(crate) const OBJECT_PATH: &str = "/openless";
#[cfg(target_os = "linux")]
pub(crate) const INTERFACE: &str = "org.fcitx.Fcitx.OpenLess1";
#[cfg(target_os = "linux")]
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcitxPluginInstallPlan {
    pub target_library: PathBuf,
    pub target_config: PathBuf,
}

impl FcitxPluginInstallPlan {
    pub fn for_layout(layout: &LinuxResourceLayout, home: &Path) -> Result<Self, BackendError> {
        let target_library = home.join(".local/lib/fcitx5/libopenless.so");
        let target_config = home.join(".local/share/fcitx5/addon/openless.conf");
        let _ = layout;
        Ok(Self {
            target_library,
            target_config,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FcitxPluginStatus {
    Ready,
    Missing,
}

pub fn ensure_plugin_installed(
    plan: &FcitxPluginInstallPlan,
) -> Result<FcitxPluginStatus, BackendError> {
    if system_plugin_available() || user_plugin_available(plan) {
        Ok(FcitxPluginStatus::Ready)
    } else {
        Ok(FcitxPluginStatus::Missing)
    }
}

fn user_plugin_available(plan: &FcitxPluginInstallPlan) -> bool {
    plan.target_library.is_file() && plan.target_config.is_file()
}

fn system_plugin_available() -> bool {
    let config_dirs = [
        std::env::var_os("FCITX5_ADDON_DIR").map(PathBuf::from),
        std::env::var_os("FCITX_ADDON_DIR").map(PathBuf::from),
        Some(PathBuf::from("/usr/share/fcitx5/addon")),
        Some(PathBuf::from("/usr/local/share/fcitx5/addon")),
    ];
    let config = config_dirs
        .into_iter()
        .flatten()
        .find(|dir| dir.join("openless.conf").is_file());
    let Some(config) = config else { return false };
    let mut library_dirs = vec![
        PathBuf::from("/usr/lib64/fcitx5"),
        PathBuf::from("/usr/lib/fcitx5"),
        PathBuf::from("/usr/local/lib/fcitx5"),
    ];
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        library_dirs.extend(entries.flatten().map(|entry| entry.path().join("fcitx5")));
    }
    if let Some(parent) = config.parent() {
        library_dirs.push(parent.to_path_buf());
    }
    library_dirs
        .iter()
        .any(|dir| dir.join("libopenless.so").is_file())
}

/// Path of the installed addon library the running fcitx5 would load: the user
/// install wins over the system one because fcitx5 searches it first.
fn installed_plugin_library(plan: &FcitxPluginInstallPlan) -> Option<PathBuf> {
    if plan.target_library.is_file() {
        return Some(plan.target_library.clone());
    }
    let mut library_dirs = vec![
        PathBuf::from("/usr/lib64/fcitx5"),
        PathBuf::from("/usr/lib/fcitx5"),
        PathBuf::from("/usr/local/lib/fcitx5"),
    ];
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        library_dirs.extend(entries.flatten().map(|entry| entry.path().join("fcitx5")));
    }
    library_dirs
        .into_iter()
        .map(|dir| dir.join("libopenless.so"))
        .find(|candidate| candidate.is_file())
}

/// `/proc/stat` -> `btime` (boot time as a UNIX timestamp in seconds).
pub(crate) fn parse_boot_time(proc_stat: &str) -> Option<u64> {
    proc_stat.lines().find_map(|line| {
        line.strip_prefix("btime ")
            .and_then(|value| value.trim().parse::<u64>().ok())
    })
}

/// `/proc/<pid>/stat` -> process start time in clock ticks since boot.
///
/// The second field is the executable name in parentheses and may contain
/// spaces, so split after the last ')' before counting fields.
pub(crate) fn parse_process_start_ticks(proc_pid_stat: &str) -> Option<u64> {
    let after_comm = proc_pid_stat.rsplit_once(')')?.1;
    let mut fields = after_comm.split_whitespace();
    // After the comm field, state is field 3; starttime is field 22 => the 20th
    // field of the remaining slice.
    fields.nth(19)?.parse::<u64>().ok()
}

/// USER_HZ for /proc values is 100 on Linux regardless of the kernel HZ.
const PROC_CLOCK_TICKS: u64 = 100;

/// True when the installed addon library is newer than the running fcitx5, i.e.
/// a package upgrade replaced the .so while the daemon still holds the old
/// image. Without a restart the new matching rules never take effect.
pub(crate) fn plugin_is_newer_than_running_fcitx5(
    plugin_modified: u64,
    boot_time: u64,
    process_start_ticks: u64,
) -> bool {
    let process_started = boot_time + process_start_ticks / PROC_CLOCK_TICKS;
    // One second of slack: both timestamps are second-resolution.
    plugin_modified > process_started.saturating_add(1)
}

/// Restart fcitx5 when the installed addon is newer than the running daemon so
/// an upgraded plugin is actually loaded. Returns true when fcitx5 was replaced.
pub fn reload_fcitx5_if_plugin_updated(plan: &FcitxPluginInstallPlan) -> bool {
    let Some(library) = installed_plugin_library(plan) else {
        return false;
    };
    let Some(modified) = std::fs::metadata(&library)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
    else {
        return false;
    };
    let (Some(proc_stat), Some(pid)) = (
        std::fs::read_to_string("/proc/stat").ok(),
        fcitx5_process_id(),
    ) else {
        return false;
    };
    let Some(proc_pid_stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok() else {
        return false;
    };
    let (Some(boot_time), Some(start_ticks)) = (
        parse_boot_time(&proc_stat),
        parse_process_start_ticks(&proc_pid_stat),
    ) else {
        return false;
    };
    if !plugin_is_newer_than_running_fcitx5(modified, boot_time, start_ticks) {
        return false;
    }
    log::info!(
        "[fcitx] addon {} is newer than the running fcitx5; restarting it to load the update",
        library.display()
    );
    reload_running_fcitx5()
}

/// PID of the running fcitx5, by scanning /proc for the process name.
#[cfg(target_os = "linux")]
fn fcitx5_process_id() -> Option<u32> {
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        let pid = name.to_string_lossy().parse::<u32>().ok();
        let Some(pid) = pid else { continue };
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        if comm.trim() == "fcitx5" {
            return Some(pid);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn fcitx5_process_id() -> Option<u32> {
    None
}

#[derive(Debug, Clone)]
pub struct Fcitx5TextInserter {
    clipboard_fallback: bool,
}

impl Fcitx5TextInserter {
    pub fn new(clipboard_fallback: bool) -> Self {
        Self { clipboard_fallback }
    }
}

impl TextInserter for Fcitx5TextInserter {
    fn begin(
        &self,
        session_id: openless_core::SessionId,
        _context: std::sync::Arc<openless_core::DictationContext>,
    ) -> BoxFuture<'static, Result<std::sync::Arc<dyn TextInsertionSession>, BackendError>> {
        let clipboard_fallback = self.clipboard_fallback;
        Box::pin(async move {
            let ticket = session_id.to_string();
            #[cfg(target_os = "linux")]
            {
                let capture_ticket = ticket.clone();
                // A missing native target is a supported clipboard fallback,
                // not permission to choose a new window after transcription.
                let _ = tokio::task::spawn_blocking(move || {
                    send_bool_message("CaptureDictationTarget", |message| {
                        message.append1(capture_ticket)
                    })
                })
                .await;
            }
            Ok(std::sync::Arc::new(Fcitx5InsertionSession {
                clipboard_fallback,
                ticket,
                closed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }) as std::sync::Arc<dyn TextInsertionSession>)
        })
    }
}

#[derive(Clone)]
struct Fcitx5InsertionSession {
    clipboard_fallback: bool,
    ticket: String,
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Fcitx5InsertionSession {
    async fn write_chunk(&self, text: String) -> Result<InsertWriteResult, BackendError> {
        #[cfg(target_os = "linux")]
        {
            let expected = text.chars().count();
            let insertion_text = text.clone();
            let closed = std::sync::Arc::clone(&self.closed);
            let ticket = self.ticket.clone();
            let result = tokio::task::spawn_blocking(move || {
                if closed.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "fcitx5 insertion session is closed",
                    ));
                }
                commit_dictation_target(&ticket, &insertion_text)
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("fcitx5 insertion task failed: {error}"),
                )
            })?;
            let written = if result.is_ok() { expected } else { 0 };
            Ok(InsertWriteResult {
                written_chars: written,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = text;
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "fcitx5 insertion is only available on Linux",
            ))
        }
    }

    async fn insert_or_copy(&self, text: String) -> Result<InsertOutcome, BackendError> {
        let clipboard_fallback = self.clipboard_fallback;
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                let insertion_text = text.clone();
                let ticket = self.ticket.clone();
                let result = tokio::task::spawn_blocking(move || {
                    commit_dictation_target(&ticket, &insertion_text)
                })
                .await
                .map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Platform,
                        format!("fcitx5 insertion task failed: {error}"),
                    )
                })?;
                if result.is_ok() {
                    return Ok(InsertOutcome::Inserted);
                }
                if clipboard_fallback {
                    tokio::task::spawn_blocking(move || copy_to_clipboard(&text))
                        .await
                        .map_err(|error| {
                            BackendError::new(
                                BackendErrorCode::Platform,
                                format!("clipboard fallback task failed: {error}"),
                            )
                        })??;
                    return Ok(InsertOutcome::CopiedFallback);
                }
                result?;
                unreachable!()
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (text, clipboard_fallback);
                Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "fcitx5 insertion is only available on Linux",
                ))
            }
        })
        .await
    }

    async fn copy_only(&self, text: String) -> Result<InsertOutcome, BackendError> {
        #[cfg(target_os = "linux")]
        {
            tokio::task::spawn_blocking(move || copy_to_clipboard(&text))
                .await
                .map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Platform,
                        format!("clipboard fallback task failed: {error}"),
                    )
                })??;
            Ok(InsertOutcome::CopiedFallback)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = text;
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "fcitx5 clipboard fallback is only available on Linux",
            ))
        }
    }
}

impl TextInsertionSession for Fcitx5InsertionSession {
    fn write(&self, text: String) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "fcitx5 insertion session is closed",
                ))
            });
        }
        let session = self.clone();
        Box::pin(async move { session.write_chunk(text).await })
    }

    fn copy(&self, text: String) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = self.clone();
        Box::pin(async move { session.copy_only(text).await.map(|_| ()) })
    }

    fn finish(
        &self,
        final_text: String,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let session = self.clone();
        Box::pin(async move {
            if session
                .closed
                .swap(true, std::sync::atomic::Ordering::AcqRel)
            {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "fcitx5 insertion session is already closed",
                ));
            }
            let result = if final_text.is_empty() {
                Ok(InsertOutcome::Inserted)
            } else {
                session.insert_or_copy(final_text).await
            };
            session.release_target().await;
            result
        })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        let session = self.clone();
        Box::pin(async move {
            session.release_target().await;
            Ok(())
        })
    }
}

impl Fcitx5InsertionSession {
    async fn release_target(&self) {
        #[cfg(target_os = "linux")]
        {
            let ticket = self.ticket.clone();
            let _ = tokio::task::spawn_blocking(move || {
                send_bool_message("CancelDictationTarget", |message| message.append1(ticket))
            })
            .await;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = &self.ticket;
    }
}

#[cfg(target_os = "linux")]
fn commit_dictation_target(ticket: &str, text: &str) -> Result<(), BackendError> {
    if send_bool_message("CommitDictationTarget", |message| {
        message.append2(ticket, text)
    })? {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::Cancelled,
            "the captured dictation target is unavailable",
        ))
    }
}

#[cfg(target_os = "linux")]
fn send_message(
    method: &str,
    append: impl FnOnce(dbus::Message) -> dbus::Message,
) -> Result<(), BackendError> {
    use dbus::blocking::BlockingSender;
    let connection = dbus::blocking::Connection::new_session().map_err(dbus_error)?;
    let message = dbus::Message::new_method_call(DESTINATION, OBJECT_PATH, INTERFACE, method)
        .map_err(|error| {
            platform_error(format!("failed to build fcitx5 {method} call: {error}"))
        })?;
    connection
        .send_with_reply_and_block(append(message), TIMEOUT)
        .map_err(dbus_error)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_bool_message(
    method: &str,
    append: impl FnOnce(dbus::Message) -> dbus::Message,
) -> Result<bool, BackendError> {
    use dbus::blocking::BlockingSender;
    let connection = dbus::blocking::Connection::new_session().map_err(dbus_error)?;
    let message = dbus::Message::new_method_call(DESTINATION, OBJECT_PATH, INTERFACE, method)
        .map_err(|error| {
            platform_error(format!("failed to build fcitx5 {method} call: {error}"))
        })?;
    let reply = connection
        .send_with_reply_and_block(append(message), TIMEOUT)
        .map_err(dbus_error)?;
    reply
        .read1::<bool>()
        .map_err(|error| platform_error(format!("invalid fcitx5 {method} reply: {error}")))
}

#[cfg(target_os = "linux")]
fn send_string_message(
    method: &str,
    append: impl FnOnce(dbus::Message) -> dbus::Message,
) -> Result<String, BackendError> {
    use dbus::blocking::BlockingSender;
    let connection = dbus::blocking::Connection::new_session().map_err(dbus_error)?;
    let message = dbus::Message::new_method_call(DESTINATION, OBJECT_PATH, INTERFACE, method)
        .map_err(|error| {
            platform_error(format!("failed to build fcitx5 {method} call: {error}"))
        })?;
    connection
        .send_with_reply_and_block(append(message), TIMEOUT)
        .map_err(dbus_error)?
        .read1::<String>()
        .map_err(|error| platform_error(format!("invalid fcitx5 {method} reply: {error}")))
}

#[cfg(target_os = "linux")]
pub(crate) fn set_raw_hotkey(method: &str, symbol: u32, states: u32) -> Result<(), BackendError> {
    send_message(method, |message| message.append2(symbol, states))
}

#[cfg(target_os = "linux")]
pub(crate) fn set_style_pack_hotkeys(
    bindings: Vec<(String, u32, u32)>,
) -> Result<(), BackendError> {
    send_message("SetStylePackHotkeys", |message| message.append1(bindings))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn set_style_pack_hotkeys(
    _bindings: Vec<(String, u32, u32)>,
) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 hotkey settings are only available on Linux",
    ))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn set_raw_hotkey(
    _method: &str,
    _symbol: u32,
    _states: u32,
) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 hotkey settings are only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn set_custom_dictation_trigger(key: &str) -> Result<(), BackendError> {
    send_message("SetCustomDictationTrigger", |message| message.append1(key))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn set_custom_dictation_trigger(_key: &str) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 hotkey settings are only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub fn commit_text(text: &str) -> Result<(), BackendError> {
    if send_bool_message("CommitText", |message| message.append1(text))? {
        Ok(())
    } else {
        Err(platform_error(
            "fcitx5 has no focused input context for text insertion".to_string(),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub fn commit_text(_: &str) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn capture_selection_target(session_id: &str) -> Result<String, BackendError> {
    send_string_message("CaptureSelectionTarget", |message| {
        message.append1(session_id)
    })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn capture_selection_target(_: &str) -> Result<String, BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn apply_selection_target(
    session_id: &str,
    source: &str,
    replacement: &str,
) -> Result<(), BackendError> {
    if send_bool_message("ApplySelectionTarget", |message| {
        message.append3(session_id, source, replacement)
    })? {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::Cancelled,
            "fcitx5 selection target changed before replacement",
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn apply_selection_target(_: &str, _: &str, _: &str) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn revert_selection_target(session_id: &str) -> Result<(), BackendError> {
    if send_bool_message("RevertSelectionTarget", |message| {
        message.append1(session_id)
    })? {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::Cancelled,
            "fcitx5 selection text changed before revert",
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn revert_selection_target(_: &str) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn cancel_selection_target(session_id: &str) -> Result<(), BackendError> {
    let _ = send_bool_message("CancelSelectionTarget", |message| {
        message.append1(session_id)
    })?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn cancel_selection_target(_: &str) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn rekey_selection_target(from: &str, to: &str) -> Result<(), BackendError> {
    if send_bool_message("RekeySelectionTarget", |message| message.append2(from, to))? {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::Cancelled,
            "fcitx5 selection target is no longer active",
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn rekey_selection_target(_: &str, _: &str) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub fn set_hotkeys(keys: Vec<String>) -> Result<(), BackendError> {
    send_message("SetHotkey", |message| message.append1(keys))
}

#[cfg(target_os = "linux")]
pub fn set_less_computer_hotkey_raw(symbol: u32, states: u32) -> Result<(), BackendError> {
    send_message("SetLessComputerHotkeyRaw", |message| {
        message.append2(symbol, states)
    })
}

#[cfg(not(target_os = "linux"))]
pub fn set_less_computer_hotkey_raw(_: u32, _: u32) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(not(target_os = "linux"))]
pub fn set_hotkeys(_: Vec<String>) -> Result<(), BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub fn selection_text() -> Result<String, BackendError> {
    use dbus::blocking::BlockingSender;
    let connection = dbus::blocking::Connection::new_session().map_err(dbus_error)?;
    let message =
        dbus::Message::new_method_call(DESTINATION, OBJECT_PATH, INTERFACE, "GetSelectionText")
            .map_err(|error| {
                platform_error(format!("failed to build fcitx5 selection call: {error}"))
            })?;
    let reply = connection
        .send_with_reply_and_block(message, TIMEOUT)
        .map_err(dbus_error)?;
    reply
        .read1::<String>()
        .map_err(|error| platform_error(format!("invalid fcitx5 selection reply: {error}")))
}

#[cfg(not(target_os = "linux"))]
pub fn selection_text() -> Result<String, BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "fcitx5 is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
pub fn available() -> bool {
    use dbus::blocking::BlockingSender;
    let Ok(connection) = dbus::blocking::Connection::new_session() else {
        return false;
    };
    let Ok(message) = dbus::Message::new_method_call(
        DESTINATION,
        OBJECT_PATH,
        "org.freedesktop.DBus.Peer",
        "Ping",
    ) else {
        return false;
    };
    connection
        .send_with_reply_and_block(message, TIMEOUT)
        .is_ok()
}

#[cfg(not(target_os = "linux"))]
pub fn available() -> bool {
    false
}

/// Ask a running fcitx5 daemon to reload so it loads a freshly written
/// OpenLess addon, mirroring the legacy Tauri `linux_fcitx` adapter.
///
/// Only an instance that currently owns the `org.fcitx.Fcitx5` DBus name is
/// restarted. On a first install fcitx5 may not be running yet; that is fine,
/// because the next fcitx5 start scans the per-user addon directory and loads
/// the addon on its own, so we never force-spawn a daemon (first-install
/// semantics are preserved). On an update the running instance is restarted so
/// the new `.so` is actually loaded (restart semantics).
///
/// Failures are logged and never fatal: startup continues down the fcitx5
/// DBus path instead of degrading to a global-hotkey fallback. Returns true
/// when a reload was issued against a live instance.
#[cfg(target_os = "linux")]
pub fn reload_running_fcitx5() -> bool {
    if !fcitx5_name_has_owner() {
        return false;
    }
    match std::process::Command::new("fcitx5").arg("-r").status() {
        Ok(status) if status.success() => {
            log::info!("[fcitx] reloaded fcitx5 after addon update");
            true
        }
        Ok(status) => {
            log::warn!("[fcitx] fcitx5 -r failed with status {status}");
            false
        }
        Err(error) => {
            log::warn!("[fcitx] could not run fcitx5 -r: {error}");
            false
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn reload_running_fcitx5() -> bool {
    false
}

/// Whether the fcitx5 daemon itself is registered on the session bus. This is
/// distinct from `available()` (which pings the OpenLess addon interface): the
/// daemon may be running without having loaded our addon yet, and that is
/// exactly the case where a reload is required.
#[cfg(target_os = "linux")]
fn fcitx5_name_has_owner() -> bool {
    use dbus::blocking::BlockingSender;
    let Ok(connection) = dbus::blocking::Connection::new_session() else {
        return false;
    };
    let Ok(message) = dbus::Message::new_method_call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
    ) else {
        return false;
    };
    connection
        .send_with_reply_and_block(message.append1(DESTINATION), Duration::from_millis(1000))
        .map(|reply| reply.read1::<bool>().unwrap_or(false))
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn copy_to_clipboard(text: &str) -> Result<(), BackendError> {
    use dbus::blocking::BlockingSender;
    let connection = dbus::blocking::Connection::new_session().map_err(dbus_error)?;
    let message =
        dbus::Message::new_method_call(DESTINATION, OBJECT_PATH, INTERFACE, "SetClipboardText")
            .map_err(|error| {
                platform_error(format!("failed to build fcitx5 clipboard call: {error}"))
            })?
            .append1(text.to_string());
    let reply = connection
        .send_with_reply_and_block(message, TIMEOUT)
        .map_err(dbus_error)?;
    if reply.read1::<bool>().unwrap_or(false) {
        Ok(())
    } else {
        Err(platform_error(
            "fcitx5 clipboard addon is unavailable".to_string(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn dbus_error(error: dbus::Error) -> BackendError {
    BackendError::new(
        BackendErrorCode::Unsupported,
        format!("fcitx5 DBus service is unavailable: {error}"),
    )
}

#[cfg(target_os = "linux")]
fn platform_error(message: String) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_time_and_process_start_are_parsed_from_proc() {
        let stat = "cpu  1 2 3\nbtime 1700000000\nprocesses 42\n";
        assert_eq!(parse_boot_time(stat), Some(1_700_000_000));
        assert_eq!(parse_boot_time("cpu 1 2 3\n"), None);

        // Field 2 is the comm in parentheses and may contain spaces/parens; the
        // 22nd field (starttime) sits 20 fields after it. Line copied from a real
        // /proc/<pid>/stat of this machine (starttime = 284037).
        let pid_stat = "38066 (bash) S 32218 38066 38066 0 -1 4194304 245 0 0 0 0 0 0 0 20 0 1 0 284037 10760192 917 18446744073709551615 93845596229632";
        assert_eq!(parse_process_start_ticks(pid_stat), Some(284037));
        // A comm containing a closing parenthesis must not shift the fields.
        let paren_comm =
            "999 (fcitx5 (5.1)) S 1 999 999 0 -1 4194304 1 0 0 0 0 0 0 0 20 0 1 0 77777 13";
        assert_eq!(parse_process_start_ticks(paren_comm), Some(77777));
        assert_eq!(parse_process_start_ticks(""), None);
        assert_eq!(parse_process_start_ticks("1 (short) S 1"), None);
    }

    #[test]
    fn a_plugin_newer_than_the_running_fcitx5_asks_for_a_restart() {
        // fcitx5 started at boot + 2500 ticks (25 s).
        let boot = 1_700_000_000;
        let started = 2500;
        // Plugin written before the daemon started: nothing to do.
        assert!(!plugin_is_newer_than_running_fcitx5(
            boot + 10,
            boot,
            started
        ));
        // Same second (the daemon read the file it just got): nothing to do.
        assert!(!plugin_is_newer_than_running_fcitx5(
            boot + 25,
            boot,
            started
        ));
        // Plugin replaced by a package upgrade while the daemon kept running.
        assert!(plugin_is_newer_than_running_fcitx5(
            boot + 600,
            boot,
            started
        ));
    }

    use super::*;

    #[test]
    fn plugin_plan_is_probe_only_for_system_packages() {
        let layout = LinuxResourceLayout {
            package_kind: crate::LinuxPackageKind::SystemPackage,
            resource_root: PathBuf::from("/usr/lib/openless/resources"),
        };
        let plan = FcitxPluginInstallPlan::for_layout(&layout, Path::new("/home/test")).unwrap();
        assert_eq!(
            plan.target_library,
            PathBuf::from("/home/test/.local/lib/fcitx5/libopenless.so")
        );
        assert_eq!(
            plan.target_config,
            PathBuf::from("/home/test/.local/share/fcitx5/addon/openless.conf")
        );
    }
}
