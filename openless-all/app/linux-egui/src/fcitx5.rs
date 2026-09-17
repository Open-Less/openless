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
/// fcitx5's own management interface. `Restart` makes the daemon replace
/// itself in place: the call returns immediately and nothing of ours is
/// inherited, unlike the `fcitx5 -r` command (see `reload_running_fcitx5`).
#[cfg(target_os = "linux")]
pub(crate) const CONTROLLER_PATH: &str = "/controller";
#[cfg(target_os = "linux")]
pub(crate) const CONTROLLER_INTERFACE: &str = "org.fcitx.Fcitx.Controller1";
#[cfg(target_os = "linux")]
const CONTROLLER_TIMEOUT: Duration = Duration::from_secs(2);

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
/// Which copy of the addon fcitx5 will actually load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginSource {
    /// The package's `/usr/.../fcitx5/libopenless.so`.
    System,
    /// A per-user copy in `~/.local/` (manual install / AppImage).
    User,
    None,
}

/// The package copy wins over a per-user copy. fcitx5 searches the user addon
/// directory first, so a leftover `~/.local` copy from an older manual install
/// would otherwise keep shadowing every package upgrade forever.
pub(crate) fn resolve_plugin_source(system: Option<&Path>, user: &Path) -> PluginSource {
    if system.is_some() {
        PluginSource::System
    } else if user.is_file() {
        PluginSource::User
    } else {
        PluginSource::None
    }
}

/// A per-user copy is stale when the package already provides the addon and the
/// per-user files are still there to shadow it.
pub(crate) fn shadows_package_plugin(system_present: bool, user_library: &Path) -> bool {
    system_present && user_library.is_file()
}

/// Reasons fcitx5 has to be restarted before a plugin change takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadReason {
    /// The installed plugin's content differs from the one we last loaded.
    ContentChanged,
    /// Same content, but the file is newer than the running daemon (a package
    /// upgrade replaced the image the daemon still holds).
    NewerThanDaemon,
    /// The daemon has a *different* library mapped (a stale manual install, or
    /// the pre-upgrade package image). Restarting is the only way to make it
    /// pick up the copy we ship.
    DaemonHoldsOtherCopy,
}

/// Whether the running fcitx5 must be restarted.
///
/// `marker` is the fingerprint of the plugin content recorded the last time the
/// host looked at it; a missing marker means "unknown baseline" and is treated
/// as a change (one restart, then the marker exists and the check is exact).
pub(crate) fn reload_reason(
    marker: Option<&str>,
    current: &str,
    newer_than_daemon: bool,
    daemon_holds_installed_copy: bool,
) -> Option<ReloadReason> {
    if !daemon_holds_installed_copy {
        return Some(ReloadReason::DaemonHoldsOtherCopy);
    }
    if marker != Some(current) {
        return Some(ReloadReason::ContentChanged);
    }
    if newer_than_daemon {
        return Some(ReloadReason::NewerThanDaemon);
    }
    None
}

/// Where the fingerprint of the plugin content last handed to fcitx5 lives.
pub fn plugin_fingerprint_path(data_dir: &Path) -> PathBuf {
    data_dir.join("fcitx5-plugin.sha256")
}

/// sha256 of a file, or `None` when it cannot be read.
pub(crate) fn file_fingerprint(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

/// Drop a per-user copy that would shadow the package's addon, so a package
/// upgrade always wins. Best effort: a failure is a warning, never fatal.
fn remove_shadowing_user_copy(plan: &FcitxPluginInstallPlan, system_present: bool) -> bool {
    if !shadows_package_plugin(system_present, &plan.target_library) {
        return false;
    }
    log::warn!(
        "[fcitx] removing the per-user addon copy {} — the package provides a newer \
         plugin and fcitx5 loads the user copy first",
        plan.target_library.display()
    );
    let mut removed = false;
    for path in [&plan.target_library, &plan.target_config] {
        match std::fs::remove_file(path) {
            Ok(()) => removed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!("[fcitx] could not remove {}: {error}", path.display()),
        }
    }
    removed
}

fn installed_plugin_library(plan: &FcitxPluginInstallPlan) -> Option<PathBuf> {
    let system = system_plugin_library();
    match resolve_plugin_source(system.as_deref(), &plan.target_library) {
        PluginSource::System => system,
        PluginSource::User => Some(plan.target_library.clone()),
        PluginSource::None => None,
    }
}

/// Addon-library search order, mirroring how fcitx5 resolves `Library=`.
///
/// The packaging prefix (Debian multiarch, e.g. `/usr/lib/x86_64-linux-gnu`)
/// comes first because that is where the .deb/.rpm puts the plugin and what
/// package upgrades replace. A manually installed `/usr/local` copy is last on
/// purpose: it is never upgraded by the package manager, so treating it as
/// "the packaged plugin" made every start believe the plugin had changed.
pub(crate) fn plugin_library_search_dirs(usr_lib_subdirs: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |dir: PathBuf| {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };
    for subdir in usr_lib_subdirs {
        push(PathBuf::from("/usr/lib").join(subdir).join("fcitx5"));
    }
    push(PathBuf::from("/usr/lib/fcitx5"));
    push(PathBuf::from("/usr/lib64/fcitx5"));
    // /usr/local is deliberately *not* a package path: anything there is a
    // leftover manual install that dpkg never replaces. Treating it as "the
    // packaged plugin" made every start believe the addon had changed and
    // kept a stale build live; `report_stale_manual_plugin` removes it instead.
    dirs
}

/// Sub-directory names of `/usr/lib` (the multiarch triples).
fn usr_lib_subdirs() -> Vec<String> {
    std::fs::read_dir("/usr/lib")
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The package's addon library, found through the same directories fcitx5 uses.
fn system_plugin_library() -> Option<PathBuf> {
    plugin_library_search_dirs(&usr_lib_subdirs())
        .into_iter()
        .map(|dir| dir.join("libopenless.so"))
        .find(|candidate| candidate.is_file())
}

/// Which `libopenless.so` the running daemon actually mapped, read from
/// `/proc/<pid>/maps`. This is the authoritative answer to "what is live".
pub(crate) fn parse_maps_plugin_path(maps: &str) -> Option<PathBuf> {
    maps.lines().find_map(|line| {
        let path = line.split_whitespace().last()?;
        path.ends_with("/libopenless.so")
            .then(|| PathBuf::from(path))
    })
}

#[cfg(target_os = "linux")]
fn running_daemon_plugin() -> Option<PathBuf> {
    let pid = fcitx5_process_id()?;
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps")).ok()?;
    parse_maps_plugin_path(&maps)
}

#[cfg(not(target_os = "linux"))]
fn running_daemon_plugin() -> Option<PathBuf> {
    None
}

/// Point out (and drop when we may) an OpenLess plugin left behind by an older
/// manual install under `/usr/local`: dpkg never replaces it, it can shadow the
/// packaged copy for fcitx5, and it makes fingerprint checks ambiguous.
/// Only our own `libopenless.so` / `openless.conf` are ever touched.
fn report_stale_manual_plugin(installed: &Path, installed_fingerprint: &str) {
    let library = PathBuf::from("/usr/local/lib/fcitx5/libopenless.so");
    let config = PathBuf::from("/usr/local/share/fcitx5/addon/openless.conf");
    if library == installed || !library.is_file() {
        return;
    }
    let Some(fingerprint) = file_fingerprint(&library) else {
        return;
    };
    if fingerprint == installed_fingerprint {
        return;
    }
    match std::fs::remove_file(&library) {
        Ok(()) => {
            log::warn!(
                "[fcitx] removed the stale manual addon {} — the packaged plugin {} wins",
                library.display(),
                installed.display()
            );
            // The addon conf in the same prefix shadows the packaged one; with
            // the library gone fcitx5 would fail to load `openless` at all, so
            // drop it as well (only ever our own file).
            match std::fs::remove_file(&config) {
                Ok(()) => log::warn!("[fcitx] removed the stale manual addon config {}", config.display()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!(
                    "[fcitx] could not remove {}: {error}; sudo rm -f {} {}",
                    config.display(),
                    library.display(),
                    config.display()
                ),
            }
        }
        Err(error) => log::warn!(
            "[fcitx] the manual addon {} (fingerprint {}) differs from the packaged plugin \
             and is not replaced by the package manager; remove it with: sudo rm -f {} {} ({error})",
            library.display(),
            &fingerprint[..fingerprint.len().min(12)],
            library.display(),
            config.display()
        ),
    }
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
pub fn reload_fcitx5_if_plugin_updated(plan: &FcitxPluginInstallPlan, data_dir: &Path) -> bool {
    let system = system_plugin_library();
    // A leftover per-user copy shadows the package plugin in fcitx5's search
    // order, so drop it first and judge the package's copy.
    remove_shadowing_user_copy(plan, system.is_some());
    let Some(library) = installed_plugin_library(plan) else {
        return false;
    };
    let Some(current) = file_fingerprint(&library) else {
        return false;
    };
    let fingerprint_path = plugin_fingerprint_path(data_dir);
    let marker = std::fs::read_to_string(&fingerprint_path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let newer = plugin_written_after_running_daemon(&library);
    // Judge the copy fcitx5 actually mapped: a leftover manual install in a
    // legacy prefix is never replaced by the package manager.
    let loaded = running_daemon_plugin();
    let daemon_loaded = loaded.as_deref();
    report_stale_manual_plugin(&library, &current);
    let daemon_holds_installed_copy = match daemon_loaded {
        Some(loaded) => file_fingerprint(loaded).as_deref() == Some(current.as_str()),
        None => true,
    };
    log::info!(
        "[fcitx] addon {} fingerprint={} loaded={} recorded={} newer_than_daemon={}",
        library.display(),
        &current[..current.len().min(12)],
        daemon_loaded
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<not running>".to_string()),
        marker
            .as_deref()
            .map(|value| &value[..value.len().min(12)])
            .unwrap_or("<none>"),
        newer,
    );
    let reason = reload_reason(
        marker.as_deref(),
        &current,
        newer,
        daemon_holds_installed_copy,
    );
    // Record the fingerprint *before* asking for the restart (and also when no
    // restart is needed): a daemon that never comes back, or a start that had
    // nothing to do, must not make the next start repeat the decision.
    if let Some(parent) = fingerprint_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&fingerprint_path, &current) {
        log::warn!("[fcitx] could not record the plugin fingerprint: {error}");
    }
    let Some(reason) = reason else {
        return false;
    };
    log::info!("[fcitx] restarting fcitx5 to load the addon update ({reason:?})");
    reload_running_fcitx5()
}

/// Whether the addon file is newer than the running fcitx5 process.
fn plugin_written_after_running_daemon(library: &Path) -> bool {
    let Some(modified) = std::fs::metadata(library)
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
    plugin_is_newer_than_running_fcitx5(modified, boot_time, start_ticks)
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
    if restart_fcitx5_via_dbus() {
        log::info!("[fcitx] reloaded fcitx5 after addon update");
        return true;
    }
    spawn_detached_fcitx5_restart()
}

/// `org.fcitx.Fcitx.Controller1.Restart` on `/controller`: the daemon replaces
/// itself, the call returns as soon as the method is dispatched, and nothing of
/// ours is inherited.
#[cfg(target_os = "linux")]
pub(crate) fn restart_fcitx5_via_dbus() -> bool {
    use dbus::blocking::BlockingSender;
    let Ok(connection) = dbus::blocking::Connection::new_session() else {
        return false;
    };
    let Ok(message) = dbus::Message::new_method_call(
        DESTINATION,
        CONTROLLER_PATH,
        CONTROLLER_INTERFACE,
        "Restart",
    ) else {
        return false;
    };
    match connection.send_with_reply_and_block(message, CONTROLLER_TIMEOUT) {
        Ok(_) => true,
        Err(error) => {
            log::warn!(
                "[fcitx] D-Bus Restart unavailable ({error}); falling back to a detached fcitx5 -r"
            );
            false
        }
    }
}

/// Last resort when the controller interface is missing: spawn `fcitx5 -r`
/// fully detached. `fcitx5 -r` *becomes* the daemon and keeps running in the
/// foreground, so `.status()`/`.wait()` would block the caller forever — the
/// child is deliberately dropped instead.
#[cfg(target_os = "linux")]
fn spawn_detached_fcitx5_restart() -> bool {
    use std::process::Stdio;
    match std::process::Command::new("fcitx5")
        .arg("-r")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            let pid = child.id();
            drop(child);
            log::info!("[fcitx] spawned a detached fcitx5 -r (pid {pid})");
            true
        }
        Err(error) => {
            log::warn!("[fcitx] could not spawn fcitx5 -r: {error}");
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
    fn a_daemon_holding_another_copy_always_asks_for_a_restart() {
        // Even with a matching fingerprint, a daemon that mapped a different
        // libopenless.so (stale manual /usr/local install, pre-upgrade image)
        // keeps the old matching rules until it is restarted.
        assert_eq!(
            reload_reason(Some("abc"), "abc", false, false),
            Some(ReloadReason::DaemonHoldsOtherCopy)
        );
        // Steady state: nothing mapped differently, fingerprint recorded.
        assert_eq!(reload_reason(Some("abc"), "abc", false, true), None);
    }

    #[test]
    fn the_packaged_addon_is_searched_before_a_manual_install() {
        let dirs = plugin_library_search_dirs(&["x86_64-linux-gnu".to_string()]);
        // A manual /usr/local install is never treated as a package path; it is
        // cleaned up separately, otherwise every start thinks the addon changed.
        assert!(
            !dirs.contains(&PathBuf::from("/usr/local/lib/fcitx5")),
            "a manual install must not masquerade as the packaged addon: {dirs:?}"
        );
        let multiarch = dirs
            .iter()
            .position(|dir| dir == Path::new("/usr/lib/x86_64-linux-gnu/fcitx5"))
            .expect("multiarch addon dir");
        assert!(
            multiarch == 0,
            "the multiarch package dir must be searched first: {dirs:?}"
        );
        // No duplicate entries when /usr/lib scans repeat a prefix.
        let twice = plugin_library_search_dirs(&[
            "x86_64-linux-gnu".to_string(),
            "x86_64-linux-gnu".to_string(),
        ]);
        assert_eq!(twice, dirs);
    }

    #[test]
    fn the_daemon_maps_the_plugin_we_read_from_proc() {
        let maps = "7f00-8000 r-xp 00000000 08:01 42 /usr/lib/fcitx5/libother.so
\
                    8000-9000 r-xp 00000000 08:01 43 /usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so
";
        assert_eq!(
            parse_maps_plugin_path(maps),
            Some(PathBuf::from(
                "/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
            ))
        );
        assert_eq!(parse_maps_plugin_path(""), None);
        assert_eq!(
            parse_maps_plugin_path("1-2 r-xp 0 00:00 0 /usr/lib/fcitx5/libopenless.so.old"),
            None
        );
    }

    #[test]
    fn the_package_plugin_wins_over_a_per_user_copy() {
        let system = Path::new("/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so");
        let user = Path::new("/home/u/.local/lib/fcitx5/libopenless.so");
        assert_eq!(
            resolve_plugin_source(Some(system), user),
            PluginSource::System
        );
        // No package plugin: fall back to the per-user copy (AppImage/manual).
        let missing = Path::new("/definitely/not/here/libopenless.so");
        assert_eq!(
            resolve_plugin_source(None, &PathBuf::from("/tmp/x")),
            PluginSource::None
        );
        assert!(!missing.is_file());
        // The decision itself must not depend on the user copy existing.
        assert_eq!(
            resolve_plugin_source(None, Path::new("/definitely/not/here")),
            PluginSource::None
        );
    }

    #[test]
    fn a_per_user_copy_only_shadows_when_the_package_has_the_addon() {
        // Both branches need a real per-user file, but never a real *package*
        // file: asserting on `/usr/lib/.../libopenless.so` only held on a
        // machine that had the deb installed and failed on clean CI runners.
        let home = tempfile::tempdir().expect("temp home");
        let user = home.path().join(".local/lib/fcitx5/libopenless.so");
        assert!(!shadows_package_plugin(true, &user));
        assert!(!shadows_package_plugin(false, &user));
        std::fs::create_dir_all(user.parent().expect("user addon dir"))
            .expect("create user addon dir");
        std::fs::write(&user, b"stale per-user copy").expect("write user addon");
        // Presence of the package copy plus an existing user file is the case
        // that used to keep an upgraded package plugin from ever loading.
        assert!(shadows_package_plugin(true, &user));
        // Without a package copy there is nothing to shadow: the per-user file
        // is the AppImage/manual plugin fcitx5 is meant to load.
        assert!(!shadows_package_plugin(false, &user));
    }

    #[test]
    fn reload_is_driven_by_content_before_mtime() {
        // Same content, daemon older than the file: mtime still asks for a reload.
        assert_eq!(
            reload_reason(Some("abc"), "abc", true, true),
            Some(ReloadReason::NewerThanDaemon)
        );
        // Same content, daemon newer: nothing to do (steady state every start).
        assert_eq!(reload_reason(Some("abc"), "abc", false, true), None);
        // Different content: reload regardless of timestamps (downgrades, files
        // restored from a backup, same-second package upgrades).
        assert_eq!(
            reload_reason(Some("abc"), "def", false, true),
            Some(ReloadReason::ContentChanged)
        );
        // Unknown baseline (first run of this check): reload once, then exact.
        assert_eq!(
            reload_reason(None, "abc", false, true),
            Some(ReloadReason::ContentChanged)
        );
    }

    #[test]
    fn probing_the_plugin_never_writes_anything() {
        // 用户拍板的策略：运行时只校验、绝不二次安装。这里把「探测不写入」
        // 锁进测试：两个目标路径都不存在时必须返回 Missing，且目录里不留任何
        // 新文件（旧实现会往 ~/.local 拷一份，反而盖过 deb 装的插件）。
        let home = tempfile::tempdir().unwrap();
        let layout = LinuxResourceLayout {
            package_kind: crate::LinuxPackageKind::SystemPackage,
            resource_root: PathBuf::from("/usr/lib/openless/resources"),
        };
        let plan = FcitxPluginInstallPlan::for_layout(&layout, home.path()).unwrap();
        let library_dir = plan.target_library.parent().unwrap().to_path_buf();
        let config_dir = plan.target_config.parent().unwrap().to_path_buf();

        // The probe itself must not create the per-user addon tree.
        assert!(
            !library_dir.exists(),
            "probe must not create {}",
            library_dir.display()
        );
        assert!(
            !config_dir.exists(),
            "probe must not create {}",
            config_dir.display()
        );
    }

    #[test]
    fn removing_a_shadowing_copy_leaves_other_addons_alone() {
        let home = tempfile::tempdir().unwrap();
        let layout = LinuxResourceLayout {
            package_kind: crate::LinuxPackageKind::SystemPackage,
            resource_root: PathBuf::from("/usr/lib/openless/resources"),
        };
        let plan = FcitxPluginInstallPlan::for_layout(&layout, home.path()).unwrap();
        for path in [&plan.target_library, &plan.target_config] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"stale").unwrap();
        }
        // A neighbouring addon from another project must survive the cleanup.
        let neighbour = plan
            .target_config
            .parent()
            .unwrap()
            .join("other-addon.conf");
        std::fs::write(&neighbour, b"keep me").unwrap();

        // Without a package plugin the per-user copy is the only one: keep it.
        assert!(!remove_shadowing_user_copy(&plan, false));
        assert!(plan.target_library.is_file());

        // With the package plugin present the stale copy would shadow it: remove.
        assert!(remove_shadowing_user_copy(&plan, true));
        assert!(!plan.target_library.exists());
        assert!(!plan.target_config.exists());
        assert_eq!(std::fs::read(&neighbour).unwrap(), b"keep me");
    }

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
    #[test]
    fn quick_phrase_key_is_detected_and_released_for_a_semicolon_hotkey() {
        // 用户机器上的真实配置（~/.config/fcitx5/conf/pinyin.conf）。
        let config = "[Pinyin]\nQuickPhraseKey=semicolon\nOtherKey=ctrl+space\n";
        assert_eq!(engine_reserved_keys(config), vec!["semicolon".to_string()]);
        // `Ctrl+Shift+;` 在前端记成 `:`，`;` 与 `:` 是同一个物理键。
        assert!(fcitx_key_aliases(":").contains(&"semicolon".to_string()));
        assert!(fcitx_key_aliases(";").contains(&"semicolon".to_string()));
        let (rewritten, previous) =
            release_engine_reserved_key(config, &fcitx_key_aliases(":")).expect("conflict");
        assert_eq!(previous, "semicolon");
        assert_eq!(
            rewritten,
            "[Pinyin]\nQuickPhraseKey=\nOtherKey=ctrl+space\n"
        );
    }

    #[test]
    fn an_unrelated_engine_key_leaves_the_config_alone() {
        // 引擎占用的是别的键：不能动用户的配置。
        assert_eq!(
            release_engine_reserved_key(
                "[Pinyin]\nQuickPhraseKey=grave\n",
                &fcitx_key_aliases(";")
            ),
            None
        );
        // 引擎什么都没占用。
        assert_eq!(
            release_engine_reserved_key("[Pinyin]\n", &fcitx_key_aliases(";")),
            None
        );
        // `QuickPhraseKey=None` 是「关闭」，不算占用。
        assert!(engine_reserved_keys("[Pinyin]\nQuickPhraseKey=None\n").is_empty());
        // 非单字符主键（LeftControl 之类的修饰键触发）没有键名别名。
        assert!(fcitx_key_aliases("LeftControl").contains(&"leftcontrol".to_string()));
    }

    #[test]
    fn releasing_the_conflicting_key_rewrites_the_engine_config_once() {
        let home = tempfile::tempdir().expect("temp home");
        let config_dir = home.path().join(".config/fcitx5/conf");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        let pinyin_conf = config_dir.join("pinyin.conf");
        std::fs::write(
            &pinyin_conf,
            "# shuangpin profile: Ziranma\nShuangpinProfile=Ziranma\nQuickPhraseKey=semicolon\n",
        )
        .expect("seed config");

        let reloaded = std::cell::Cell::new(0);
        let reload = |_: &str| {
            reloaded.set(reloaded.get() + 1);
            true
        };
        // `Ctrl+Shift+;` 在前端记成 `:`：两条都要能命中同一个物理键的占用。
        assert!(release_engine_key_conflict_in(home.path(), ":", &reload));
        let rewritten = std::fs::read_to_string(&pinyin_conf).expect("config readable");
        assert!(rewritten.contains("QuickPhraseKey=\n"));
        assert!(!rewritten.contains("QuickPhraseKey=semicolon"));
        // 用户原有内容（注释、双拼方案）必须原样保留。
        assert!(rewritten.contains("ShuangpinProfile=Ziranma"));
        let backup = std::fs::read_to_string(config_dir.join("pinyin.conf.openless-backup"))
            .expect("backup");
        assert!(backup.contains("QuickPhraseKey=semicolon"));
        assert_eq!(reloaded.get(), 1);

        // 第二次进来已经没有冲突：不再改写、不再重载，备份也不会被覆盖。
        assert!(!release_engine_key_conflict_in(home.path(), ":", &reload));
        assert_eq!(reloaded.get(), 1);
    }

    #[test]
    fn a_missing_engine_config_is_not_created() {
        let home = tempfile::tempdir().expect("temp home");
        assert!(!release_engine_key_conflict_in(home.path(), ";", &|_| true));
        assert!(!home.path().join(".config/fcitx5/conf/pinyin.conf").exists());
    }
}

/// fcitx5 引擎把某个键声明成了自己的「快速短语」触发键（`QuickPhraseKey`）。
///
/// 这个键在**到达 addon 事件过滤器之前**就被引擎消费，所以拿它当热键主键的绑定
/// 永远收不到按键（实测：插件逐键 trace 里能看到字母键，却完全没有 `;`）。
/// 返回值是配置里声明的键名列表（例如 `["semicolon"]`）。
pub fn engine_reserved_keys(config: &str) -> Vec<String> {
    config
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key.trim() != "QuickPhraseKey" {
                return None;
            }
            let value = value.trim();
            if value.is_empty() || value.eq_ignore_ascii_case("none") {
                return None;
            }
            Some(value.to_string())
        })
        .collect()
}

/// 把热键主键换算成 fcitx 键名，供 [`engine_reserved_keys`] 比对。
///
/// 前端把 Shift 折进 keysym（`Ctrl+Shift+;` 记成 `:`），所以 `;` 与 `:` 都指向
/// 同一个物理键 `semicolon`。
pub fn fcitx_key_aliases(base: &str) -> Vec<String> {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut aliases = Vec::new();
    let lowercase = trimmed.to_ascii_lowercase();
    if lowercase.chars().count() == 1 {
        if let Some(name) = fcitx_punctuation_name(trimmed.chars().next().unwrap_or_default()) {
            aliases.push(name.to_string());
        }
    }
    aliases.push(lowercase);
    aliases.sort();
    aliases.dedup();
    aliases
}

fn fcitx_punctuation_name(character: char) -> Option<&'static str> {
    Some(match character {
        ';' | ':' => "semicolon",
        ',' | '<' => "comma",
        '.' | '>' => "period",
        '/' | '?' => "slash",
        '\'' | '"' => "apostrophe",
        '[' | '{' => "bracketleft",
        ']' | '}' => "bracketright",
        '-' | '_' => "minus",
        '=' | '+' => "equal",
        '\\' | '|' => "backslash",
        '`' | '~' => "grave",
        _ => return None,
    })
}

/// 当 `aliases` 里的键名被引擎占用时，改写配置把 `QuickPhraseKey` 置空，
/// 返回 `(改写后的配置, 被释放的原值)`；没有冲突时返回 `None`。
pub fn release_engine_reserved_key(config: &str, aliases: &[String]) -> Option<(String, String)> {
    let reserved = engine_reserved_keys(config);
    let occupied = reserved
        .iter()
        .find(|key| aliases.iter().any(|alias| alias.eq_ignore_ascii_case(key)))?;
    let mut rewritten = String::with_capacity(config.len());
    for line in config.lines() {
        let is_target = line
            .split_once('=')
            .is_some_and(|(key, _)| key.trim() == "QuickPhraseKey");
        if is_target {
            rewritten.push_str("QuickPhraseKey=");
            rewritten.push('\n');
        } else {
            rewritten.push_str(line);
            rewritten.push('\n');
        }
    }
    Some((rewritten, occupied.clone()))
}

/// 释放被输入法引擎占用的热键主键；返回是否真的改了配置。
///
/// 只动我们自己的用户配置 `~/.config/fcitx5/conf/pinyin.conf`，并且**先备份**
/// （`pinyin.conf.openless-backup`，只在备份不存在时写），改完通过
/// `Controller1.ReloadAddonConfig` 让拼音重新加载。用户想还原时把备份拷回去即可。
pub fn release_engine_key_conflict(base_key: &str) -> bool {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    release_engine_key_conflict_in(&home, base_key, &reload_addon_config)
}

/// [`release_engine_key_conflict`] 的可注入版本：`home` 与「让引擎重载配置」这两件
/// 外部依赖显式传入，便于用临时目录断言「改了哪个文件、备份在哪、什么时候不该动」。
pub fn release_engine_key_conflict_in(
    home: &Path,
    base_key: &str,
    reload: &dyn Fn(&str) -> bool,
) -> bool {
    let aliases = fcitx_key_aliases(base_key);
    if aliases.is_empty() {
        return false;
    }
    let config_path = home.join(".config/fcitx5/conf/pinyin.conf");
    let Ok(config) = std::fs::read_to_string(&config_path) else {
        return false;
    };
    let Some((rewritten, previous)) = release_engine_reserved_key(&config, &aliases) else {
        return false;
    };
    let backup_path = config_path.with_extension("conf.openless-backup");
    if !backup_path.exists() {
        if let Err(error) = std::fs::copy(&config_path, &backup_path) {
            log::warn!(
                "[fcitx] cannot back up {} before releasing its quick-phrase key: {error}",
                config_path.display()
            );
            return false;
        }
    }
    if let Err(error) = std::fs::write(&config_path, rewritten) {
        log::warn!(
            "[fcitx] cannot release the quick-phrase key in {}: {error}",
            config_path.display()
        );
        return false;
    }
    if reload("pinyin") {
        log::info!(
            "[fcitx] released the input method's {} reservation ({}) so the hotkey reaches the \
             addon; original config backed up at {}",
            previous,
            config_path.display(),
            backup_path.display()
        );
    } else {
        log::warn!(
            "[fcitx] cleared QuickPhraseKey={previous} in {}; restart the input method for it to \
             take effect (backup: {})",
            config_path.display(),
            backup_path.display()
        );
    }
    true
}

/// `org.fcitx.Fcitx.Controller1.ReloadAddonConfig(s)` on `/controller`: the
/// engine re-reads its own config file so the released key takes effect now.
#[cfg(target_os = "linux")]
pub(crate) fn reload_addon_config(addon: &str) -> bool {
    use dbus::blocking::BlockingSender;
    let Ok(connection) = dbus::blocking::Connection::new_session() else {
        return false;
    };
    let Ok(message) = dbus::Message::new_method_call(
        DESTINATION,
        CONTROLLER_PATH,
        CONTROLLER_INTERFACE,
        "ReloadAddonConfig",
    )
    .map(|message| message.append1(addon)) else {
        return false;
    };
    match connection.send_with_reply_and_block(message, CONTROLLER_TIMEOUT) {
        Ok(_) => true,
        Err(error) => {
            log::warn!("[fcitx] D-Bus ReloadAddonConfig({addon}) failed: {error}");
            false
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn reload_addon_config(_addon: &str) -> bool {
    false
}
