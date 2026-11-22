use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex as StdMutex};

#[derive(Default)]
struct PlaybackState {
    active: Option<(String, Arc<AtomicBool>)>,
    stopped: VecDeque<String>,
}
impl PlaybackState {
    fn begin(&mut self, request_id: String) -> Option<Arc<AtomicBool>> {
        if self.stopped.contains(&request_id) {
            return None;
        }
        let token = Arc::new(AtomicBool::new(false));
        if let Some((_, old)) = self.active.replace((request_id, Arc::clone(&token))) {
            old.store(true, Ordering::Release);
        }
        Some(token)
    }
    fn stop(&mut self, request_id: String) {
        // A synchronous Stop command can arrive before the async Start task runs.
        // Retain bounded cancellation markers so that task cannot revive playback.
        if !self.stopped.contains(&request_id) {
            self.stopped.push_back(request_id.clone());
            if self.stopped.len() > 64 {
                self.stopped.pop_front();
            }
        }
        if self
            .active
            .as_ref()
            .is_some_and(|(id, _)| id == &request_id)
        {
            if let Some((_, token)) = self.active.take() {
                token.store(true, Ordering::Release);
            }
        }
    }
}
static PLAYBACK: LazyLock<StdMutex<HashMap<String, PlaybackState>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));
struct PlaybackGuard {
    owner: String,
    token: Arc<AtomicBool>,
}
impl Drop for PlaybackGuard {
    fn drop(&mut self) {
        let mut map = PLAYBACK.lock().unwrap();
        if let Some(state) = map.get_mut(&self.owner) {
            if state
                .active
                .as_ref()
                .is_some_and(|(_, token)| Arc::ptr_eq(token, &self.token))
            {
                state.active = None;
            }
        }
    }
}
#[tauri::command]
pub fn deepseek_tts_stop(window: Window, request_id: String) {
    if !matches!(window.label(), "qa" | "main") || request_id.len() > 128 {
        return;
    }
    PLAYBACK
        .lock()
        .unwrap()
        .entry(window.label().into())
        .or_default()
        .stop(request_id);
}
#[tauri::command]
pub async fn deepseek_tts_start(
    window: Window,
    core: CoreState<'_>,
    text: String,
    request_id: String,
    audio: tauri::ipc::Channel<Vec<u8>>,
) -> Result<(), String> {
    if !matches!(window.label(), "qa" | "main") {
        return Err("qa_window_required".into());
    }
    if core.get_preferences().pipeline_mode != openless_core::shared_types::PipelineMode::Deepseek {
        return Err("Read aloud is available only in DeepSeek suite mode".into());
    }
    if request_id.is_empty() || request_id.len() > 128 {
        return Err("Invalid speech request id".into());
    }
    let Some(token) = PLAYBACK
        .lock()
        .unwrap()
        .entry(window.label().into())
        .or_default()
        .begin(request_id)
    else {
        return Ok(());
    };
    let _guard = PlaybackGuard {
        owner: window.label().into(),
        token: token.clone(),
    };
    // Resolve the channel identity and key from one vault snapshot so a channel
    // repurposed for another vendor cannot send that vendor's secret to DeepSeek.
    let (secret, voice) =
        tokio::task::spawn_blocking(CredentialsVault::get_deepseek_tts_credentials)
            .await
            .map_err(|_| "Could not read speech configuration")?
            .map_err(|_| "Configure an official DeepSeek channel in Model services")?;
    let cancelled = async {
        while !token.load(Ordering::Acquire)
            && core.get_preferences().pipeline_mode
                == openless_core::shared_types::PipelineMode::Deepseek
        {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    };
    tokio::select! {
        biased;
        _ = cancelled => Ok(()),
        result = tokio::time::timeout(std::time::Duration::from_secs(1800), openless_core::deepseek_tts::stream(secret.expose_secret(), &text, &voice, |bytes| audio.send(bytes).map_err(|_| "Playback window closed".into()))) => result.map_err(|_| "Speech playback timed out".to_string())?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_before_start_prevents_late_network_work() {
        let mut state = PlaybackState::default();
        state.stop("cancelled".into());
        assert!(state.begin("cancelled".into()).is_none());
        assert!(state.active.is_none());
    }
    #[test]
    fn late_stop_cannot_cancel_a_new_request() {
        let mut state = PlaybackState::default();
        let first = state.begin("first".into()).unwrap();
        let second = state.begin("second".into()).unwrap();
        assert!(first.load(Ordering::Acquire));
        state.stop("first".into());
        assert!(!second.load(Ordering::Acquire));
        state.stop("second".into());
        assert!(second.load(Ordering::Acquire));
        assert!(state.begin("second".into()).is_none());
        for i in 0..100 {
            state.stop(format!("request-{i}"));
        }
        assert_eq!(state.stopped.len(), 64);
    }
}
