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

/// 每日活动计数（日期升序），概览页年度热力图的数据源。与历史内容 / 保留策略解耦：
/// 清空历史不影响它，全年格子照亮。
#[tauri::command]
pub fn get_activity_stats(coord: CoordinatorState<'_>) -> Vec<ActivityDay> {
    coord
        .activity()
        .snapshot()
        .into_iter()
        .map(|(date, count)| ActivityDay { date, count })
        .collect()
}

/// 读取某次会话的原始麦克风 wav 字节流。文件存在的条件：debug 用户的任意会话，或任意
/// 「转录失败 / empty」会话（失败保留）——成功的非 debug 会话录音会在插入后删掉。
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

/// 对一条「转录失败」历史条目的归档录音用**当前** ASR provider 重新转录（issue #613）。
///
/// 流程：读 `recordings/<id>.wav` → 取 PCM（跳过 44 字节 WAV 头）→ 现 provider 重转
/// → 成功则原地回写该条历史的 rawTranscript / finalText、清除 error_code，返回新文本。
///
/// 仅做 ASR，不自动二次润色（润色依赖 LLM 凭据且 issue 标为待定，留作后续）。失败时
/// 不动历史、不删录音，把错误返回给前端提示，用户可重试。返回更新后的整条记录给前端
/// 局部刷新。
#[tauri::command]
pub async fn retranscribe_recording(
    coord: CoordinatorState<'_>,
    session_id: String,
) -> Result<DictationSession, String> {
    if !is_valid_session_id(&session_id) {
        return Err("invalid session id".into());
    }
    let path =
        crate::persistence::recording_path_for_session(&session_id).map_err(|e| e.to_string())?;
    let wav = tokio::fs::read(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "recording not found".into()
        } else {
            format!("read wav failed: {e}")
        }
    })?;
    // 归档 wav 是 16k/mono/16-bit、固定 44 字节标准头（见 asr::wav::encode_wav_16k_mono）。
    if wav.len() <= 44 {
        return Err("recording is empty or corrupt".into());
    }
    let pcm = wav[44..].to_vec();

    let text = coord.retranscribe_pcm(pcm).await?;
    if text.trim().is_empty() {
        return Err("重新转录仍未识别到语音".into());
    }

    // 找到原条目，保留其它字段，只更新转写结果 + 清错误码。
    let mut entry = coord
        .history()
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "history entry not found".to_string())?;
    // 只更新转写结果并清除失败标记。insert_status 保持原值（重新转录不向光标落字，
    // 没有可表达「已转写未落字」的状态，清掉 error_code 即足以标记不再是失败条目）。
    entry.raw_transcript = text.clone();
    entry.final_text = text;
    entry.error_code = None;

    let updated = coord
        .history()
        .update_entry(entry.clone())
        .map_err(|e| e.to_string())?;
    if !updated {
        return Err("history entry not found".into());
    }
    Ok(entry)
}
