use super::*;

#[tauri::command]
pub fn list_history(coord: CoordinatorState<'_>) -> Result<Vec<DictationSession>, String> {
    coord.history().list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_history_entry(coord: CoordinatorState<'_>, id: String) -> Result<(), String> {
    coord.history().delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.history().clear().map_err(|e| e.to_string())
}

/// #613: 对一条 ASR 转录失败的历史条目，用归档的 WAV 文件重新转写。
///
/// 工作流：
/// 1. 校验 session_id（UUID-v4 白名单）
/// 2. 从 history JSON 查找原条目
/// 3. 读取归档 WAV → 解码为 PCM
/// 4. 按当前 `active_asr_provider` 构造 ASR 并转写
/// 5. 成功 → 更新历史条目（rawTranscript + 清除 errorCode）；失败 → 返回错误，原条目不修改
///
/// 目前支持的 ASR 提供商：
/// - Whisper / MiMo（HTTP batch）— 直接 transcribe PCM
/// - 其他（Volcengine/Bailian/本地模型）— 暂时返回 "unsupported provider" 错误
#[tauri::command]
pub async fn retranscribe_history_entry(
    session_id: String,
) -> Result<DictationSession, String> {
    if !is_valid_session_id(&session_id) {
        return Err("invalid session id".into());
    }

    // Read WAV file — use non-CoordinatorState path (standalone command)
    let wav_path = crate::persistence::recording_path_for_session(&session_id)
        .map_err(|e| e.to_string())?;
    if !wav_path.exists() {
        return Err("recording not found".into());
    }
    let wav_bytes = tokio::fs::read(&wav_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "recording not found".into()
        } else {
            format!("read wav failed: {e}")
        }
    })?;

    // Decode WAV → PCM bytes (16-bit little-endian interleaved)
    let pcm_samples =
        crate::asr::wav::decode_wav_to_pcm_i16(&wav_bytes)?;
    let pcm_bytes: Vec<u8> = pcm_samples
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();

    // Determine ASR provider from prefs
    let prefs = crate::persistence::PreferencesStore::new()
        .map_err(|e| e.to_string())?
        .get();
    let provider = &prefs.active_asr_provider;

    let raw = transcribe_pcm_from_wav(&pcm_bytes, provider).await?;

    // Update the history entry
    let history = crate::persistence::HistoryStore::new().map_err(|e| e.to_string())?;
    let Some(mut entry) = history.find_entry(&session_id).map_err(|e| e.to_string())? else {
        return Err("history entry not found".into());
    };
    entry.raw_transcript = raw.text;
    entry.error_code = None;
    history
        .update_entry(&session_id, entry.clone())
        .map_err(|e| e.to_string())?;

    Ok(entry)
}

/// Core transcription engine dispatch: pick the right ASR provider based on the
/// `active_asr_provider` string and call its batch transcription method with raw PCM.
async fn transcribe_pcm_from_wav(
    pcm: &[u8],
    provider: &str,
) -> Result<crate::asr::RawTranscript, String> {
    match provider {
        "whisper" => {
            let creds = crate::coordinator::read_whisper_credentials();
            let asr = crate::asr::WhisperBatchASR::new(
                creds.0,
                creds.1,
                creds.2,
                None,  // prompt: retranscribe uses None — hotword context unavailable
                None,  // no chunk limit
                false, // verbose_json: false for retranscribe
            );
            asr.transcribe_pcm(pcm).await.map_err(|e| e.to_string())
        }
        "mimo" => {
            let creds = crate::coordinator::read_mimo_credentials();
            let asr = crate::asr::MimoBatchASR::new(creds.0, creds.1, creds.2);
            asr.transcribe_pcm(pcm).await.map_err(|e| e.to_string())
        }
        // All other providers currently unsupported for file-based retranscription.
        // See issue #613 discussion: Volcengine/Bailian use WebSocket streaming,
        // local models need runtime access that isn't available from this standalone command.
        _ => Err(format!(
            "当前 ASR 提供商 \"{provider}\" 不支持文件重转录。请切换到 Whisper 或 MiMo 后重试。"
        )),
    }
}

/// 读取某次会话的原始麦克风 wav 字节流。仅当用户开过
/// `prefs.record_audio_for_debug` 并且这条 session 是开关打开后录的，才会有文件。
/// 文件名规约：`<data_dir>/recordings/<session_id>.wav`，与 DictationSession.id 同名。
///
/// 路径校验：session_id **必须**严格匹配 UUID-v4 字面（36 字符 = 8-4-4-4-12 + 4 个 `-`，
/// 内容仅 ASCII 十六进制 + `-`）。白名单胜过黑名单——绝对路径前缀、Windows ADS、
/// 百分号编码、NUL 字节都不在合法字符集里，挡掉所有 Path::join 越界的可能。
/// session_id 在仓库内由 `Uuid::new_v4()` 生成 (`dictation.rs:1531`)，前端只会回传
/// 自己列出的合法 id，但 IPC = boundary，按 boundary 规则严格校验。
///
/// async fs：单条 5 分钟 wav 约 9.6MB，同步 `std::fs::read` 会阻塞 Tauri IPC 主循环。
/// 改 `tokio::fs::read` 后让出线程给其它 IPC。
#[tauri::command]
pub async fn read_audio_recording(session_id: String) -> Result<Vec<u8>, String> {
    if !is_valid_session_id(&session_id) {
        return Err("invalid session id".into());
    }
    let path =
        crate::persistence::recording_path_for_session(&session_id).map_err(|e| e.to_string())?;
    if !path.exists() {
        return Err("recording not found".into());
    }
    // TOCTOU 兜底：exists() 通过到 read 之间文件可能被 prune（条数 cap / retention
    // 清理 / 用户手动删）。把 NotFound 标准化成跟 exists() 失败同样的错误字符串，
    // 前端单条 'recording not found' catch 就能稳定隐藏按钮，不依赖本地化 OS 错误。
    tokio::fs::read(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "recording not found".into()
        } else {
            format!("read wav failed: {e}")
        }
    })
}
