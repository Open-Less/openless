//! Adapter from the existing coherent capsule snapshot to a disposable egui UI.
use crate::capsule_mirrors::{self, MonitorRect};
use crate::types::{CapsuleDisplayMode, CapsulePayload, CapsuleState, CapsuleStyle};
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
}
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
        })
    }
    pub(crate) fn set_locale(&self, locale: String) {
        *self.locale.lock() = locale;
    }
    pub(crate) fn update_transcript(&self, session_id: &str, text: String) {
        let coordinator = self.app.state::<Arc<crate::coordinator::Coordinator>>();
        let enabled = coordinator
            .backend()
            .get_preferences()
            .capsule_transcript_enabled;
        let mut latest = self.latest.lock();
        if let Some(frame) = latest.frame.as_mut() {
            if frame.update_transcript(session_id, if enabled { text } else { String::new() }) {
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
        *self.latest.lock() = Presentation::default();
        self.layout_epoch.fetch_add(1, Ordering::Relaxed);
        for supervisor in self.supervisors.lock().iter() {
            supervisor.stop();
        }
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
        let transcript = if preferences.capsule_transcript_enabled
            && matches!(
                payload.state,
                CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing
            ) {
            transcript
        } else {
            String::new()
        };
        let frame = Frame {
            session_id: payload.session_id.clone().unwrap_or_default(),
            sequence,
            state: CapsulePopupState {
                phase: phase.into(),
                text: payload.message.clone().unwrap_or_default(),
                audio_level: (!payload.warming).then_some(payload.level),
                translation_active: payload.translation,
                style: style.into(),
            },
            transcript,
            transcript_font_size: preferences.capsule_transcript_font_size as f32,
            labels: CapsuleLabels::for_locale(&self.locale.lock()),
            bounds: None,
            interactive,
        };
        if !frame.visible() {
            self.stop();
            return;
        }
        let mode = crate::tauri_coordinator_host::normalized_display_mode(
            preferences.capsule_display_mode,
        );
        let mut latest = self.latest.lock();
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

#[tauri::command]
pub(crate) fn native_capsule_set_locale(app: tauri::AppHandle, locale: String) {
    if !matches!(
        locale.as_str(),
        "zh-CN" | "zh-TW" | "en" | "ja" | "ko" | "es" | "fr" | "de"
    ) {
        return;
    }
    // Save even in regular mode: enabling low-memory mode then restarting
    // silently must keep the UI language chosen before the restart.
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
