use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::time::Duration;

use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, InsertOutcome, InsertWriteResult, ResourceResolver,
    TextInserter, TextInsertionSession,
};

use crate::{LinuxPackageKind, LinuxResourceLayout, FCITX_PLUGIN_CONFIG, FCITX_PLUGIN_LIBRARY};

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
    pub source_library: Option<PathBuf>,
    pub source_config: Option<PathBuf>,
    pub target_library: PathBuf,
    pub target_config: PathBuf,
    pub copy_required: bool,
}

impl FcitxPluginInstallPlan {
    pub fn for_layout(layout: &LinuxResourceLayout, home: &Path) -> Result<Self, BackendError> {
        let target_library = home.join(".local/lib/fcitx5/libopenless.so");
        let target_config = home.join(".local/share/fcitx5/addon/openless.conf");
        if layout.package_kind == LinuxPackageKind::AppImage {
            let resolver = layout.resolver()?;
            Ok(Self {
                source_library: Some(resolver.resolve(Path::new(FCITX_PLUGIN_LIBRARY))?),
                source_config: Some(resolver.resolve(Path::new(FCITX_PLUGIN_CONFIG))?),
                target_library,
                target_config,
                copy_required: true,
            })
        } else {
            Ok(Self {
                source_library: None,
                source_config: None,
                target_library,
                target_config,
                copy_required: false,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FcitxPluginStatus {
    Ready,
    Missing,
    Updated,
}

pub fn ensure_plugin_installed(
    plan: &FcitxPluginInstallPlan,
) -> Result<FcitxPluginStatus, BackendError> {
    if !plan.copy_required {
        return if system_plugin_available() || user_plugin_available(plan) {
            Ok(FcitxPluginStatus::Ready)
        } else {
            Ok(FcitxPluginStatus::Missing)
        };
    }
    let source_library = plan.source_library.as_ref().ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            "AppImage plugin plan is missing the bundled library",
        )
    })?;
    let source_config = plan.source_config.as_ref().ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            "AppImage plugin plan is missing the bundled config",
        )
    })?;
    let library = read_non_empty(source_library)?;
    let config = read_non_empty(source_config)?;
    let library_changed = target_differs(&plan.target_library, &library)?;
    let config_changed = target_differs(&plan.target_config, &config)?;
    if !library_changed && !config_changed {
        return Ok(FcitxPluginStatus::Ready);
    }
    if library_changed {
        atomic_write(&plan.target_library, &library, true)?;
    }
    if config_changed {
        atomic_write(&plan.target_config, &config, false)?;
    }
    Ok(FcitxPluginStatus::Updated)
}

fn read_non_empty(path: &Path) -> Result<Vec<u8>, BackendError> {
    let bytes = std::fs::read(path).map_err(|error| {
        BackendError::new(
            BackendErrorCode::Platform,
            format!("failed to read fcitx5 resource {}: {error}", path.display()),
        )
    })?;
    if bytes.is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::Platform,
            format!("fcitx5 resource {} is empty", path.display()),
        ));
    }
    Ok(bytes)
}

fn target_differs(path: &Path, expected: &[u8]) -> Result<bool, BackendError> {
    match std::fs::read(path) {
        Ok(actual) => Ok(actual != expected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(BackendError::new(
            BackendErrorCode::Platform,
            format!(
                "failed to read existing fcitx5 file {}: {error}",
                path.display()
            ),
        )),
    }
}

fn atomic_write(path: &Path, bytes: &[u8], executable: bool) -> Result<(), BackendError> {
    let parent = path.parent().ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            "fcitx5 target has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        BackendError::new(
            BackendErrorCode::Platform,
            format!("failed to create fcitx5 target directory: {error}"),
        )
    })?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("openless"),
        std::process::id()
    ));
    std::fs::write(&temporary, bytes).map_err(|error| {
        BackendError::new(
            BackendErrorCode::Platform,
            format!("failed to stage fcitx5 resource: {error}"),
        )
    })?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755)).map_err(
            |error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("failed to set fcitx5 plugin permissions: {error}"),
                )
            },
        )?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    // POSIX rename replaces an existing file atomically, so the previously
    // working plugin remains available if staging or commit fails. The
    // non-Unix branch exists only for portable unit tests/tooling, where the
    // platform rename API may require explicitly removing the destination.
    #[cfg(not(unix))]
    if path.exists() {
        std::fs::remove_file(path).map_err(|error| {
            BackendError::new(
                BackendErrorCode::Platform,
                format!("failed to replace fcitx5 resource: {error}"),
            )
        })?;
    }
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        BackendError::new(
            BackendErrorCode::Platform,
            format!("failed to commit fcitx5 resource: {error}"),
        )
    })
}

fn user_plugin_available(plan: &FcitxPluginInstallPlan) -> bool {
    plan.target_library.is_file() && plan.target_config.is_file()
}

fn system_plugin_available() -> bool {
    let library = [
        "/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so",
        "/usr/lib64/fcitx5/libopenless.so",
        "/usr/lib/fcitx5/libopenless.so",
    ]
    .iter()
    .any(|path| Path::new(path).is_file());
    library && Path::new("/usr/share/fcitx5/addon/openless.conf").is_file()
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

#[cfg(target_os = "linux")]
fn copy_to_clipboard(text: &str) -> Result<(), BackendError> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| platform_error(format!("failed to open Linux clipboard: {error}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| platform_error(format!("failed to write Linux clipboard: {error}")))
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
    fn appimage_plan_copies_only_from_the_versioned_resource_contract() {
        let layout = LinuxResourceLayout {
            package_kind: LinuxPackageKind::AppImage,
            resource_root: PathBuf::from("/app/usr/lib/openless/resources"),
        };
        let plan = FcitxPluginInstallPlan::for_layout(&layout, Path::new("/home/test")).unwrap();
        assert!(plan.copy_required);
        assert_eq!(
            plan.source_library.unwrap(),
            PathBuf::from("/app/usr/lib/openless/resources/linux-fcitx5-plugin/libopenless.so")
        );
        assert_eq!(
            plan.target_config,
            PathBuf::from("/home/test/.local/share/fcitx5/addon/openless.conf")
        );
    }

    #[test]
    fn system_packages_never_copy_bundled_plugins_into_home() {
        let layout = LinuxResourceLayout {
            package_kind: LinuxPackageKind::SystemPackage,
            resource_root: PathBuf::from("/usr/lib/openless/resources"),
        };
        let plan = FcitxPluginInstallPlan::for_layout(&layout, Path::new("/home/test")).unwrap();
        assert!(!plan.copy_required);
        assert!(plan.source_library.is_none());
        assert!(plan.source_config.is_none());
    }

    #[test]
    fn appimage_installer_copies_then_reports_ready() {
        let root = std::env::temp_dir().join(format!(
            "openless-fcitx-appimage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let resources = root.join("resources");
        let home = root.join("home");
        std::fs::create_dir_all(resources.join("linux-fcitx5-plugin")).unwrap();
        std::fs::write(resources.join(FCITX_PLUGIN_LIBRARY), b"plugin").unwrap();
        std::fs::write(resources.join(FCITX_PLUGIN_CONFIG), b"config").unwrap();
        let plan = FcitxPluginInstallPlan::for_layout(
            &LinuxResourceLayout {
                package_kind: LinuxPackageKind::AppImage,
                resource_root: resources,
            },
            &home,
        )
        .unwrap();

        assert_eq!(
            ensure_plugin_installed(&plan).unwrap(),
            FcitxPluginStatus::Updated
        );
        assert_eq!(std::fs::read(&plan.target_library).unwrap(), b"plugin");
        assert_eq!(std::fs::read(&plan.target_config).unwrap(), b"config");
        assert_eq!(
            ensure_plugin_installed(&plan).unwrap(),
            FcitxPluginStatus::Ready
        );

        std::fs::write(plan.source_library.as_ref().unwrap(), b"updated plugin").unwrap();
        assert_eq!(
            ensure_plugin_installed(&plan).unwrap(),
            FcitxPluginStatus::Updated
        );
        assert_eq!(
            std::fs::read(&plan.target_library).unwrap(),
            b"updated plugin"
        );
        assert_eq!(std::fs::read(&plan.target_config).unwrap(), b"config");

        let _ = std::fs::remove_dir_all(root);
    }
}
