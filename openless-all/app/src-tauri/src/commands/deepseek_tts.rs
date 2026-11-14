use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex as StdMutex};

static PLAYBACK: LazyLock<StdMutex<HashMap<String, (String, Arc<AtomicBool>)>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));
struct PlaybackGuard {
    owner: String,
    token: Arc<AtomicBool>,
}
impl Drop for PlaybackGuard {
    fn drop(&mut self) {
        let mut map = PLAYBACK.lock().unwrap();
        if map
            .get(&self.owner)
            .is_some_and(|current| Arc::ptr_eq(&current.1, &self.token))
        {
            map.remove(&self.owner);
        }
    }
}
#[tauri::command]
pub fn deepseek_tts_stop(window: Window, request_id: String) {
    let mut map = PLAYBACK.lock().unwrap();
    if map
        .get(window.label())
        .is_some_and(|entry| entry.0 == request_id)
    {
        if let Some((_, token)) = map.remove(window.label()) {
            token.store(true, Ordering::Release);
        }
    }
}
#[tauri::command]
pub async fn deepseek_tts_start(
    window: Window,
    text: String,
    request_id: String,
    audio: tauri::ipc::Channel<Vec<u8>>,
) -> Result<(), String> {
    if !matches!(window.label(), "qa" | "main") {
        return Err("qa_window_required".into());
    }
    let token = Arc::new(AtomicBool::new(false));
    if let Some(old) = PLAYBACK
        .lock()
        .unwrap()
        .insert(window.label().into(), (request_id, token.clone()))
    {
        old.1.store(true, Ordering::Release);
    }
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
        while !token.load(Ordering::Acquire) {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    };
    tokio::select! {
        _ = cancelled => Ok(()),
        result = tokio::time::timeout(std::time::Duration::from_secs(1800), openless_core::deepseek_tts::stream(secret.expose_secret(), &text, &voice, |bytes| audio.send(bytes).map_err(|_| "Playback window closed".into()))) => result.map_err(|_| "Speech playback timed out".to_string())?,
    }
}
