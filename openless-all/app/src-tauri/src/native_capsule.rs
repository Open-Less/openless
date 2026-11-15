//! Adapter from the existing coherent capsule snapshot to a disposable egui UI.
use crate::capsule_mirrors::{self, MonitorRect};
use crate::types::{CapsuleDisplayMode, CapsulePayload, CapsuleState, CapsuleStyle, ThemeMode};
use openless_capsule::{
    supervisor::CapsuleSupervisor, Action, CapsuleLabels, CapsulePopupState, Frame,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tauri::{Emitter, Manager};

#[derive(Default)]
struct Presentation {
    frame: Option<Frame>,
    mode: CapsuleDisplayMode,
    bounds: Vec<Option<[i32; 4]>>,
    foreground: Option<MonitorRect>,
    foreground_captured: bool,
}

impl Presentation {
    fn update_theme(&mut self, theme: openless_capsule::ThemeMode) -> bool {
        let Some(frame) = self.frame.as_mut() else {
            return false;
        };
        if frame.theme == theme {
            return false;
        }
        frame.theme = theme;
        true
    }
}

pub(crate) struct NativeCapsule {
    executable: std::path::PathBuf,
    on_action: Arc<dyn Fn(String, Action) + Send + Sync>,
    on_health_change: Arc<dyn Fn() + Send + Sync>,
    supervisors: parking_lot::Mutex<Vec<CapsuleSupervisor>>,
    app: tauri::AppHandle,
    locale: parking_lot::Mutex<String>,
    latest: parking_lot::Mutex<Presentation>,
    position_pending: AtomicBool,
    layout_epoch: AtomicU64,
    /// Bumped by every presentation and stop; a pending exit only reaps the
    /// helpers if nothing was shown after it started.
    exit_epoch: AtomicU64,
    /// (native target name, UI locale) -> localized target name.
    translation_name: parking_lot::Mutex<Option<(String, String, String)>>,
}

/// `DEFAULT_CAPSULE_EXIT_MS` (180) plus a frame: the default capsule's exit
/// motion plays on the helper before the host reaps it.
const EXIT_MS: u64 = 200;

impl NativeCapsule {
    pub(crate) fn new(app: tauri::AppHandle) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let helper_name = format!("openless-native-capsule{suffix}");
        // Tauri externalBin lives next to the host in installed .app/Windows
        // bundles. Dev builds may use the workspace target directory.
        let packaged = executable
            .parent()
            .ok_or("executable has no directory")?
            .join(&helper_name);
        let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/debug")
            .join(&helper_name);
        let path = if packaged.is_file() { packaged } else { dev };
        if !path.is_file() {
            return Err(format!(
                "native capsule helper missing: {} (run npm run build:native-capsule)",
                path.display()
            ));
        }
        let app_for_action = app.clone();
        let action: Arc<dyn Fn(String, Action) + Send + Sync> =
            Arc::new(move |session_id: String, action: Action| {
                let app = app_for_action.clone();
                tauri::async_runtime::spawn(async move {
                    let coordinator = app.state::<Arc<crate::coordinator::Coordinator>>();
                    let backend = coordinator.backend();
                    // Recheck Core ownership after the process/async hop. A delayed
                    // action can never operate on a newly started dictation.
                    let snapshot = backend.snapshot().dictation;
                    if snapshot.session_id.map(|id| id.to_string()).as_deref()
                        != Some(session_id.as_str())
                    {
                        return;
                    }
                    match action {
                        Action::Cancel => {
                            coordinator.cancel_active_voice().await;
                        }
                        Action::Confirm
                            if snapshot.phase == openless_core::DictationPhase::Recording =>
                        {
                            let _ = backend.stop_dictation().await;
                        }
                        _ => {}
                    }
                });
            });
        let handle = tauri::async_runtime::handle().inner().clone();
        let health_app = app.clone();
        let on_health_change: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = health_app.emit("native-capsule:health-changed", ());
        });
        let supervisor = CapsuleSupervisor::spawn(
            &handle,
            path.clone(),
            Arc::clone(&action),
            Arc::clone(&on_health_change),
        );
        let saved_locale = app
            .path()
            .app_config_dir()
            .ok()
            .and_then(|path| std::fs::read_to_string(path.join("native-capsule.locale")).ok());
        let locale = saved_locale
            .unwrap_or_else(|| std::env::var("LANG").unwrap_or_else(|_| "en".into()))
            .replace('_', "-");
        Ok(Self {
            executable: path,
            on_action: action,
            on_health_change,
            supervisors: parking_lot::Mutex::new(vec![supervisor]),
            app,
            locale: parking_lot::Mutex::new(locale),
            latest: parking_lot::Mutex::new(Presentation::default()),
            position_pending: AtomicBool::new(false),
            layout_epoch: AtomicU64::new(0),
            exit_epoch: AtomicU64::new(0),
            translation_name: parking_lot::Mutex::new(None),
        })
    }
    pub(crate) fn set_locale(&self, locale: String) {
        *self.locale.lock() = locale;
    }
    fn current_theme(&self) -> openless_capsule::ThemeMode {
        let coordinator = self.app.state::<Arc<crate::coordinator::Coordinator>>();
        match coordinator.backend().get_preferences().theme_mode {
            ThemeMode::System => openless_capsule::ThemeMode::System,
            ThemeMode::Light => openless_capsule::ThemeMode::Light,
            ThemeMode::Dark => openless_capsule::ThemeMode::Dark,
        }
    }
    /// Restyle an existing helper without replaying recording state or reversing its exit.
    pub(crate) fn refresh_theme(&self) {
        let mut latest = self.latest.lock();
        if latest.update_theme(self.current_theme()) {
            self.send_frames(&latest);
        }
    }
    /// The text also feeds the "transcribing" label, so it is sent even when
    /// the transcript rail is turned off (`Frame::show_transcript`).
    pub(crate) fn update_transcript(&self, session_id: &str, text: String) {
        let mut latest = self.latest.lock();
        if let Some(frame) = latest.frame.as_mut() {
            if frame.update_transcript(session_id, text) {
                self.send_frames(&latest);
            }
        }
    }
    pub(crate) fn last_error(&self) -> Option<String> {
        self.supervisors
            .lock()
            .iter()
            .find_map(CapsuleSupervisor::last_error)
    }
    pub(crate) fn stop(&self) {
        let mut latest = self.latest.lock();
        self.stop_locked(&mut latest);
    }
    fn stop_locked(&self, latest: &mut Presentation) {
        *latest = Presentation::default();
        self.exit_epoch.fetch_add(1, Ordering::SeqCst);
        self.layout_epoch.fetch_add(1, Ordering::Relaxed);
        for supervisor in self.supervisors.lock().iter() {
            supervisor.stop();
        }
    }
    /// Replays the last frame as its exit and reaps the helpers once the
    /// motion is over. The spectral style dismisses immediately.
    fn leave(self: &Arc<Self>, sequence: u64) {
        let mut latest = self.latest.lock();
        let exiting = latest.frame.as_ref().is_some_and(|frame| frame.leaving);
        let animated = latest
            .frame
            .as_ref()
            .is_some_and(|frame| frame.state.style != "siri")
            && !latest.bounds.is_empty();
        if exiting {
            return;
        }
        if !animated {
            self.stop_locked(&mut latest);
            return;
        }
        latest.frame = latest.frame.take().map(|frame| frame.into_leaving(sequence));
        self.send_frames(&latest);
        let epoch = self.exit_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        drop(latest);
        let native = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(EXIT_MS)).await;
            let mut latest = native.latest.lock();
            if native.exit_epoch.load(Ordering::SeqCst) == epoch {
                native.stop_locked(&mut latest);
            }
        });
    }
    /// The session's target language, named in the UI language as the
    /// WebView badge does with `Intl.DisplayNames`.
    fn translation_target(&self, saved: &str) -> String {
        let coordinator = self.app.state::<Arc<crate::coordinator::Coordinator>>();
        let target = coordinator
            .backend()
            .dictation_translation_target()
            .map(|target| target.target_language)
            .filter(|target| !target.trim().is_empty())
            .unwrap_or_else(|| saved.to_owned());
        let native =
            openless_core::language_catalog::native_language_name(&target).unwrap_or(target);
        let locale = self.locale.lock().clone();
        let mut cache = self.translation_name.lock();
        if let Some((cached, cached_locale, name)) = cache.as_ref() {
            if *cached == native && *cached_locale == locale {
                return name.clone();
            }
        }
        let name = openless_core::language_catalog::language_code(&native)
            .and_then(|code| localized_language_name(&code, &locale))
            .unwrap_or_else(|| native.clone());
        *cache = Some((native, locale, name.clone()));
        name
    }
    pub(crate) fn present(
        self: &Arc<Self>,
        payload: &CapsulePayload,
        sequence: u64,
        transcript: String,
    ) {
        let coordinator = self.app.state::<Arc<crate::coordinator::Coordinator>>();
        let preferences = coordinator.backend().get_preferences();
        if !preferences.show_capsule && !payload.selection_polish {
            self.stop();
            return;
        }
        let phase = match payload.state {
            CapsuleState::Idle => "idle",
            CapsuleState::Recording => "recording",
            CapsuleState::Transcribing => "transcribing",
            CapsuleState::Polishing => "polishing",
            CapsuleState::Done => "completed",
            CapsuleState::Cancelled => "cancelled",
            CapsuleState::Error => "failed",
        };
        let interactive = matches!(
            payload.state,
            CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing
        ) && coordinator
            .backend()
            .snapshot()
            .dictation
            .session_id
            .is_some_and(|id| Some(id.to_string()) == payload.session_id);
        let style = match preferences.capsule_style {
            CapsuleStyle::Siri => "siri",
            CapsuleStyle::Classic => "classic",
            CapsuleStyle::Typeless => "typeless",
        };
        let transcript = if matches!(
            payload.state,
            CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing
        ) {
            transcript
        } else {
            String::new()
        };
        let mut frame = Frame {
            session_id: payload.session_id.clone().unwrap_or_default(),
            sequence,
            state: CapsulePopupState {
                phase: phase.into(),
                text: payload.message.clone().unwrap_or_default(),
                audio_level: (!payload.warming).then_some(payload.level),
                translation_active: payload.translation,
                style: style.into(),
                warming: payload.warming && payload.state == CapsuleState::Recording,
                operating: payload.operating,
                inserted_chars: payload.inserted_chars,
                selection_polish: payload.selection_polish,
                translation_target: payload
                    .translation
                    .then(|| self.translation_target(&preferences.translation_target_language)),
            },
            transcript,
            show_transcript: preferences.capsule_transcript_enabled,
            transcript_font_size: preferences.capsule_transcript_font_size as f32,
            labels: CapsuleLabels::for_locale(&self.locale.lock()),
            theme: match preferences.theme_mode {
                ThemeMode::System => openless_capsule::ThemeMode::System,
                ThemeMode::Light => openless_capsule::ThemeMode::Light,
                ThemeMode::Dark => openless_capsule::ThemeMode::Dark,
            },
            leaving: false,
            bounds: None,
            interactive,
        };
        if !frame.visible() {
            self.leave(sequence);
            return;
        }
        let mode = crate::tauri_coordinator_host::normalized_display_mode(
            preferences.capsule_display_mode,
        );
        let mut latest = self.latest.lock();
        // Serialize with preference refreshes so an older pending frame cannot
        // restore the previous theme after a settings event.
        frame.theme = self.current_theme();
        // A new presentation reverses a running exit on the same helpers.
        self.exit_epoch.fetch_add(1, Ordering::SeqCst);
        let was_idle = latest.frame.is_none();
        let needs_position = latest.mode != mode
            || latest.frame.as_ref().is_none_or(|previous| {
                previous.session_id != frame.session_id
                    || (frame.state.phase == "recording" && previous.state.phase != "recording")
            });
        if needs_position {
            latest.bounds.clear();
            latest.foreground = None;
            latest.foreground_captured = false;
        }
        latest.mode = mode;
        latest.frame = Some(frame);
        if !latest.bounds.is_empty() {
            self.send_frames(&latest);
        }
        drop(latest);
        if needs_position {
            self.request_layout();
        }
        if was_idle {
            let epoch = self.layout_epoch.fetch_add(1, Ordering::Relaxed) + 1;
            let native = Arc::clone(self);
            // Continue following pointer/topology changes after microphone frames stop.
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if native.layout_epoch.load(Ordering::Relaxed) != epoch {
                        break;
                    }
                    native.request_layout();
                }
            });
        }
    }

    fn request_layout(self: &Arc<Self>) {
        if self.position_pending.swap(true, Ordering::Relaxed) {
            return;
        }
        let native = Arc::clone(self);
        if let Err(error) = self.app.run_on_main_thread(move || {
            native.refresh_layout();
            native.position_pending.store(false, Ordering::Relaxed);
        }) {
            self.position_pending.store(false, Ordering::Relaxed);
            log::warn!("[native-capsule] monitor placement failed: {error}");
        }
    }

    fn refresh_layout(&self) {
        let mut latest = self.latest.lock();
        if latest.frame.is_none() {
            return;
        }
        let coordinator = self.app.state::<Arc<crate::coordinator::Coordinator>>();
        let mode = crate::tauri_coordinator_host::normalized_display_mode(
            coordinator.backend().get_preferences().capsule_display_mode,
        );
        if latest.mode != mode {
            latest.mode = mode;
            latest.foreground = None;
            latest.foreground_captured = false;
        }
        let monitors: Vec<_> = self
            .app
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(MonitorRect::from_tauri)
            .collect();
        let primary = self
            .app
            .primary_monitor()
            .ok()
            .flatten()
            .as_ref()
            .map(MonitorRect::from_tauri);
        let cursor = self
            .app
            .cursor_position()
            .ok()
            .map(|point| (point.x as i32, point.y as i32));
        #[cfg(windows)]
        if latest.mode == CapsuleDisplayMode::ForegroundWindowScreen
            && !latest.foreground_captured
            && latest
                .frame
                .as_ref()
                .is_some_and(|frame| frame.state.phase == "recording")
        {
            latest.foreground = coordinator
                .tauri_host()
                .capsule_window()
                .and_then(|capsule| capsule.capture_foreground_monitor());
            latest.foreground_captured = true;
        }
        latest.bounds = capsule_mirrors::display_targets(
            latest.mode,
            &monitors,
            primary,
            cursor,
            latest.foreground,
        )
        .into_iter()
        .map(|monitor| {
            let (width, height, x, y) =
                capsule_mirrors::capsule_placement(460.0, 158.0, 12.0, monitor);
            Some([x, y, width as i32, height as i32])
        })
        .collect();
        if latest.bounds.is_empty() {
            latest.bounds.push(None);
        }
        self.send_frames(&latest);
    }

    fn send_frames(&self, presentation: &Presentation) {
        let Some(frame) = &presentation.frame else {
            return;
        };
        let mut supervisors = self.supervisors.lock();
        // The same bounded mirror policy is used by regular WebView capsules.
        let previous_error = supervisors.iter().find_map(CapsuleSupervisor::last_error);
        supervisors.truncate(presentation.bounds.len());
        if previous_error != supervisors.iter().find_map(CapsuleSupervisor::last_error) {
            (self.on_health_change)();
        }
        while supervisors.len() < presentation.bounds.len() {
            supervisors.push(CapsuleSupervisor::spawn(
                tauri::async_runtime::handle().inner(),
                self.executable.clone(),
                Arc::clone(&self.on_action),
                Arc::clone(&self.on_health_change),
            ));
        }
        for (supervisor, bounds) in supervisors.iter().zip(&presentation.bounds) {
            let mut frame = frame.clone();
            frame.bounds = *bounds;
            supervisor.present(frame);
        }
    }
}

/// `[[NSLocale localeWithLocaleIdentifier:locale] localizedStringForLocaleIdentifier:code]`,
/// the same CLDR "standard" name `Intl.DisplayNames` gives the WebView.
#[cfg(target_os = "macos")]
fn localized_language_name(code: &str, locale: &str) -> Option<String> {
    use objc2::{msg_send, runtime::AnyClass, runtime::AnyObject};
    use objc2_foundation::NSString;
    let class = AnyClass::get("NSLocale")?;
    let (identifier, code) = (NSString::from_str(locale), NSString::from_str(code));
    objc2::rc::autoreleasepool(|_| unsafe {
        // SAFETY: both selectors take an NSString and return autoreleased
        // objects that are copied out before the pool drains.
        let ns_locale: *mut AnyObject = msg_send![class, localeWithLocaleIdentifier: &*identifier];
        if ns_locale.is_null() {
            return None;
        }
        let name: *mut NSString = msg_send![ns_locale, localizedStringForLocaleIdentifier: &*code];
        name.as_ref()
            .map(|name| name.to_string())
            .filter(|name| !name.is_empty())
    })
}

/// Windows has no per-call display locale; the native name is shown instead.
#[cfg(not(target_os = "macos"))]
fn localized_language_name(_code: &str, _locale: &str) -> Option<String> {
    None
}

#[tauri::command]
pub(crate) fn native_capsule_set_locale(app: tauri::AppHandle, locale: String) {
    if !matches!(
        locale.as_str(),
        "zh-CN" | "zh-TW" | "en" | "ja" | "ko" | "es" | "fr" | "de"
    ) {
        return;
    }
    // Persist the helper locale so it survives app restarts.
    if let Ok(directory) = app.path().app_config_dir() {
        if std::fs::create_dir_all(&directory).is_ok() {
            let temporary = directory.join("native-capsule.locale.tmp");
            if std::fs::write(&temporary, locale.as_bytes()).is_ok() {
                // Windows rename does not replace an existing target.
                let target = directory.join("native-capsule.locale");
                let _ = std::fs::remove_file(&target);
                let _ = std::fs::rename(temporary, target);
            }
        }
    }
    if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
        coordinator.tauri_host().set_native_capsule_locale(locale);
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    #[test]
    fn a_theme_change_preserves_the_active_session_and_exit_state() {
        for phase in ["recording", "polishing", "completed"] {
            for leaving in [false, true] {
                let frame = Frame {
                    session_id: "theme-session".into(),
                    sequence: 42,
                    state: CapsulePopupState {
                        phase: phase.into(),
                        audio_level: Some(0.4),
                        ..Default::default()
                    },
                    transcript: "keep this transcript".into(),
                    show_transcript: true,
                    transcript_font_size: 15.0,
                    labels: CapsuleLabels::for_locale("zh-CN"),
                    theme: openless_capsule::ThemeMode::Light,
                    leaving,
                    bounds: Some([10, 20, 460, 158]),
                    interactive: !leaving,
                };
                let mut expected = frame.clone();
                expected.theme = openless_capsule::ThemeMode::Dark;
                let mut presentation = Presentation {
                    frame: Some(frame),
                    bounds: vec![Some([10, 20, 460, 158])],
                    ..Default::default()
                };
                assert!(presentation.update_theme(openless_capsule::ThemeMode::Dark));
                assert_eq!(presentation.frame.as_ref(), Some(&expected));
                assert_eq!(presentation.bounds, vec![Some([10, 20, 460, 158])]);
                assert!(!presentation.update_theme(openless_capsule::ThemeMode::Dark));
                assert!(presentation.update_theme(openless_capsule::ThemeMode::System));
            }
        }
    }

    #[test]
    fn a_theme_change_does_not_reopen_an_idle_capsule() {
        let mut presentation = Presentation::default();
        assert!(!presentation.update_theme(openless_capsule::ThemeMode::Dark));
        assert!(presentation.frame.is_none());
        assert!(presentation.bounds.is_empty());
    }
}
