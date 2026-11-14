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
    core: CoreState<'_>,
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
    let config_key = openless_core::CredentialKey::new(
        openless_core::CredentialNamespace::Application,
        None,
        "web_search.config",
    )
    .map_err(|e| e.message)?;
    let raw = core
        .read_credential(config_key)
        .await
        .map_err(|e| e.message)?;
    let config: openless_core::web_search::WebSearchConfig = raw
        .map(|s| serde_json::from_str(s.expose_secret()))
        .transpose()
        .map_err(|_| "Invalid speech configuration")?
        .unwrap_or_default();
    if config.tts_channel.is_empty() {
        return Err("Configure DeepSeek suite in Model services to enable read aloud".into());
    }
    let key = openless_core::CredentialKey::new(
        openless_core::CredentialNamespace::Llm,
        Some(config.tts_channel),
        "ark.api_key",
    )
    .map_err(|e| e.message)?;
    let secret = core
        .read_credential(key)
        .await
        .map_err(|_| "Could not read DeepSeek API key")?
        .ok_or("DeepSeek API key is missing")?;
    let cancelled = async {
        while !token.load(Ordering::Acquire) {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    };
    tokio::select! {
        _ = cancelled => Ok(()),
        result = tokio::time::timeout(std::time::Duration::from_secs(1800), openless_core::deepseek_tts::stream(secret.expose_secret(), &text, &config.voice, |bytes| audio.send(bytes).map_err(|_| "Playback window closed".into()))) => result.map_err(|_| "Speech playback timed out".to_string())?,
    }
}
