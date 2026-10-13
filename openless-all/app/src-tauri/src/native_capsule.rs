//! Adapter from the existing coherent capsule snapshot to a disposable egui UI.
use crate::types::{CapsulePayload, CapsuleState, CapsuleStyle};
use openless_capsule::{
    supervisor::CapsuleSupervisor, Action, CapsuleLabels, CapsulePopupState, Frame,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::Manager;

pub(crate) struct NativeCapsule {
    supervisor: CapsuleSupervisor,
    app: tauri::AppHandle,
    locale: parking_lot::Mutex<String>,
    latest: parking_lot::Mutex<Option<Frame>>,
    position_pending: AtomicBool,
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
        let action = Arc::new(move |session_id: String, action: Action| {
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
        let supervisor = CapsuleSupervisor::spawn(&handle, path, action);
        let saved_locale = app
            .path()
            .app_config_dir()
            .ok()
            .and_then(|path| std::fs::read_to_string(path.join("native-capsule.locale")).ok());
        let locale = saved_locale
            .unwrap_or_else(|| std::env::var("LANG").unwrap_or_else(|_| "en".into()))
            .replace('_', "-");
        Ok(Self {
            supervisor,
            app,
            locale: parking_lot::Mutex::new(locale),
            latest: parking_lot::Mutex::new(None),
            position_pending: AtomicBool::new(false),
        })
    }
    pub(crate) fn set_locale(&self, locale: String) {
        *self.locale.lock() = locale;
    }
    pub(crate) fn stop(&self) {
        self.latest.lock().take();
        self.position_pending.store(false, Ordering::Relaxed);
        self.supervisor.stop();
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
        let interactive = coordinator
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
        let session_id = frame.session_id.clone();
        let mut latest = self.latest.lock();
        let needs_position = latest
            .as_ref()
            .is_none_or(|previous| previous.session_id != session_id);
        let mut frame = frame;
        if !needs_position {
            frame.bounds = latest.as_ref().and_then(|previous| previous.bounds);
        }
        *latest = Some(frame.clone());
        drop(latest);
        if needs_position {
            self.position_pending.store(true, Ordering::Relaxed);
            let native = Arc::clone(self);
            // Monitor/window APIs stay on the main thread. First-frame volume
            // updates are coalesced in `latest` until position is available.
            if let Err(error) = self.app.run_on_main_thread(move || {
                let bounds = target_bounds(&native.app);
                let mut latest = native.latest.lock();
                if let Some(frame) = latest
                    .as_mut()
                    .filter(|frame| frame.session_id == session_id)
                {
                    frame.bounds = bounds;
                    native.position_pending.store(false, Ordering::Relaxed);
                    native.supervisor.present(frame.clone());
                }
            }) {
                log::warn!("[native-capsule] monitor placement failed: {error}");
                self.position_pending.store(false, Ordering::Relaxed);
                if let Some(frame) = self.latest.lock().as_ref() {
                    self.supervisor.present(frame.clone());
                }
            }
        } else if !self.position_pending.load(Ordering::Relaxed) {
            self.supervisor.present(frame);
        }
    }
}

fn target_bounds(app: &tauri::AppHandle) -> Option<[i32; 4]> {
    // Host APIs are used without constructing a hidden WebView just to discover
    // a monitor. Pointer monitor is the existing macOS capsule policy.
    #[cfg(windows)]
    if let Some(monitor) = crate::foreground_window_monitor() {
        let width = (460.0 * monitor.scale).round() as i32;
        let height = (158.0 * monitor.scale).round() as i32;
        return Some([
            monitor.work_left + (monitor.work_right - monitor.work_left - width) / 2,
            monitor.work_bottom - height - (12.0 * monitor.scale) as i32,
            width,
            height,
        ]);
    }
    let point = app.cursor_position().ok()?;
    let monitor = app
        .monitor_from_point(point.x, point.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let width = (460.0 * scale).round() as i32;
    let height = (158.0 * scale).round() as i32;
    Some([
        area.position.x + (area.size.width as i32 - width) / 2,
        area.position.y + area.size.height as i32 - height - (12.0 * scale) as i32,
        width,
        height,
    ])
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
