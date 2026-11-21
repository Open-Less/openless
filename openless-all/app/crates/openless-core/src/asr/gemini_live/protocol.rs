//! Gemini Live wire messages and replaceable transcript snapshots.

use anyhow::{bail, Result};
use base64::Engine;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use super::GeminiLiveConfig;
use crate::asr::gemini::TranscriptionMode;

/// One recording is one manual activity, so pauses cannot finish dictation early.
pub(super) fn setup(config: &GeminiLiveConfig) -> Message {
    let languages = config
        .languages
        .iter()
        .filter_map(|value| {
            crate::asr::gemini::language_code(value).map(|code| match code.as_str() {
                "zh" | "zh-CN" => "cmn-Hans-CN".to_owned(),
                "yue" | "zh-HK" => "yue-Hant-HK".to_owned(),
                _ => code,
            })
        })
        .collect::<Vec<_>>();
    let vocabulary = crate::asr::gemini::normalize_vocabulary(config.hotwords.clone());
    let mode = match config.mode {
        TranscriptionMode::Verbatim => "VERBATIM",
        TranscriptionMode::Smart => "SMART",
    };
    Message::text(
        json!({"setup": {
            "model": format!("models/{}", super::MODEL),
            "generationConfig": {"responseModalities": ["TEXT"]},
            "realtimeInputConfig": {"automaticActivityDetection": {"disabled": true}},
            "inputAudioTranscription": {
                "mode": mode, "languageCodes": languages, "customVocabulary": vocabulary
            }
        }})
        .to_string(),
    )
}

/// Send raw canonical PCM; WAV headers are never part of the audio stream.
pub(super) fn audio(pcm: &[u8]) -> Message {
    Message::text(
        json!({"realtimeInput": {"audio": {
            "data": base64::engine::general_purpose::STANDARD.encode(pcm),
            "mimeType": "audio/pcm;rate=16000"
        }}})
        .to_string(),
    )
}

/// Manual VAD uses activity markers instead of audioStreamEnd.
pub(super) fn activity(start: bool) -> Message {
    let field = if start {
        "activityStart"
    } else {
        "activityEnd"
    };
    Message::text(json!({"realtimeInput": {field: {}}}).to_string())
}

/// Google sends binary JSON; compatible gateways may send text JSON.
pub(super) fn decode(message: Message) -> Result<Option<Value>> {
    let value: Value = match message {
        Message::Text(text) => serde_json::from_str(&text),
        Message::Binary(bytes) => serde_json::from_slice(&bytes),
        Message::Close(_) => bail!("Gemini Live disconnected before transcription completed"),
        _ => return Ok(None),
    }
    .map_err(|_| anyhow::anyhow!("invalid Gemini Live server message"))?;
    if value.get("error").is_some() {
        // Server messages may contain credentials or signed URLs; expose only the code.
        let code = value["error"]["code"].as_u64().unwrap_or_default();
        bail!("Gemini Live rejected the transcription request (code {code})");
    }
    if value.get("goAway").is_some() {
        bail!("Gemini Live session is expiring; retry the recording");
    }
    Ok(Some(value))
}

/// Finalized segments and the current speculative hypothesis have separate lifetimes.
#[derive(Default)]
pub(super) struct Transcript {
    completed: Vec<String>,
    interim: String,
    final_received: bool,
    generation_complete: bool,
    activity_ended: bool,
}

impl Transcript {
    /// Ignore completion markers from before the recording's stop boundary.
    pub(super) fn begin_finish(&mut self) {
        self.generation_complete = false;
        self.activity_ended = false;
    }

    /// Publish snapshots, replacing interim text and preserving repeated final phrases.
    pub(super) fn update(&mut self, value: &Value, finishing: bool) -> Option<String> {
        let before = self.preview();
        let content = &value["serverContent"];
        if let Some(text) = content["interimInputTranscription"]["text"].as_str() {
            self.interim = text.to_owned();
        }
        if let Some(text) = content["inputTranscription"]["text"].as_str() {
            if !text.trim().is_empty() {
                self.completed.push(text.to_owned());
            }
            self.interim.clear();
            self.final_received = true;
        }
        if finishing {
            self.generation_complete |= content["generationComplete"] == true;
            self.activity_ended |= value["voiceActivity"]["type"] == "ACTIVITY_END";
        }
        let after = self.preview();
        (before != after).then_some(after)
    }

    fn preview(&self) -> String {
        let mut segments = self.completed.clone();
        if !self.interim.is_empty() {
            segments.push(self.interim.clone());
        }
        crate::asr::qwen_realtime::join_segments(&segments)
    }

    /// Completion markers permit draining, but unresolved speculative text must still be finalized.
    pub(super) fn can_finish(&self, silence: bool) -> bool {
        self.interim.is_empty()
            && ((self.final_received && (self.generation_complete || self.activity_ended))
                || (silence && self.activity_ended))
    }

    /// Speculative text is never promoted to a successful final result.
    pub(super) fn final_text(&self) -> String {
        crate::asr::qwen_realtime::join_segments(&self.completed)
    }
}
