use super::*;
use tauri_plugin_dialog::{DialogExt, FilePath};

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
/// 读取录音文件的 data URL（base64），前端 `<audio>` 直接 `src={url}` 播放。
///
/// 之前的实现返回 `Vec<u8>`，Tauri IPC 将其 JSON 序列化为 number 数组（~460 KB），
/// 在 WebKit/Wry 中 `<audio>` 解析这个 Blob 有时会失败（表现为时长 0、导出无反应）。
/// 改用 base64 data URL 后：
/// - IPC payload 只增大 33%（150 KB），仍在安全范围内
/// - 前端不需要 `ArrayBuffer → Blob → createObjectURL` 的复杂链路
/// - 导出按钮直接把 data URL 设为 `<a>.href` 即可触发浏览器下载
#[tauri::command]
pub async fn read_audio_recording(session_id: String) -> Result<String, String> {
    if !is_valid_session_id(&session_id) {
        return Err("invalid session id".into());
    }
    let path =
        crate::persistence::recording_path_for_session(&session_id).map_err(|e| e.to_string())?;
    let data = tokio::fs::read(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "recording not found".into()
        } else {
            format!("read wav failed: {e}")
        }
    })?;
    log::info!("[history] read_audio_recording id={session_id} bytes={} head={:?}", data.len(), &data.get(..16));
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
    let data_url = format!("data:audio/wav;base64,{b64}");
    log::info!("[history] read_audio_recording data_url_len={}", data_url.len());
    Ok(data_url)
}

/// 把已归档录音 wav 导出到用户选定的路径。
///
/// 后端直接调系统文件保存对话框，路径不经 IPC 传递，无法被篡改或注入。
/// 对话框调用在 spawn_blocking 中执行，避免阻塞 Tauri 异步线程池。
#[tauri::command]
pub async fn export_audio_recording(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<String, String> {
    if !is_valid_session_id(&session_id) {
        return Err("invalid session id".into());
    }

    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let file_path = app
            .dialog()
            .file()
            .add_filter("WAV audio", &["wav"])
            .set_file_name(format!("openless-recording-{session_id}.wav"))
            .blocking_save_file();

        let Some(file_path) = file_path else {
            return Err("user cancelled".into());
        };

        let src = crate::persistence::recording_path_for_session(&session_id)
            .map_err(|e| e.to_string())?;

        export_recording_to_destination(&app, file_path, &src)
    })
    .await
    .map_err(|e| format!("internal error: {e}"))?
}

fn export_recording_to_destination(
    app: &tauri::AppHandle,
    file_path: FilePath,
    source: &std::path::Path,
) -> Result<String, String> {
    #[cfg(target_os = "android")]
    if let FilePath::Url(url) = &file_path {
        if url.scheme() == "content" {
            copy_recording_to_mobile_url(app, &file_path, source)?;
            return Ok(url.to_string());
        }
    }

    #[cfg(target_os = "ios")]
    if let FilePath::Url(url) = &file_path {
        if url.scheme() == "file" {
            copy_recording_to_mobile_url(app, &file_path, source)?;
            return Ok(url.to_string());
        }
    }

    let destination = file_path
        .into_path()
        .map_err(export_recording_failed)?;
    copy_recording_to_path(source, &destination)?;
    Ok(destination.to_string_lossy().into_owned())
}

const RECORDING_EXPORT_FAILED: &str = "recording export failed";

fn open_recording_source(source: &std::path::Path) -> Result<std::fs::File, String> {
    std::fs::File::open(source).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "recording not found".to_string()
        } else {
            export_recording_failed(error)
        }
    })
}

fn export_recording_failed(error: impl std::fmt::Display) -> String {
    log::error!("[history] audio recording export failed: {error}");
    RECORDING_EXPORT_FAILED.to_string()
}

fn copy_recording_to_path(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    let mut source_file = open_recording_source(source)?;
    let mut destination_file = std::fs::File::create(destination).map_err(export_recording_failed)?;
    std::io::copy(&mut source_file, &mut destination_file)
        .map(|_| ())
        .map_err(export_recording_failed)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn copy_recording_to_mobile_url(
    app: &tauri::AppHandle,
    destination: &FilePath,
    source: &std::path::Path,
) -> Result<(), String> {
    use tauri_plugin_fs::{FsExt, OpenOptions};

    let mut source_file = open_recording_source(source)?;
    let mut options = OpenOptions::new();
    options.write(true).truncate(true).create(true);
    let mut destination_file = match app.fs().open(destination.clone(), options) {
        Ok(file) => file,
        Err(error) => {
            #[cfg(target_os = "ios")]
            let _ = app.fs().stop_accessing_security_scoped_resource(destination.clone());
            return Err(export_recording_failed(error));
        }
    };

    let copy_result = std::io::copy(&mut source_file, &mut destination_file)
        .map(|_| ())
        .map_err(export_recording_failed);

    #[cfg(target_os = "ios")]
    let stop_result = app
        .fs()
        .stop_accessing_security_scoped_resource(destination.clone())
        .map_err(export_recording_failed);

    if let Err(error) = copy_result {
        #[cfg(target_os = "ios")]
        let _ = stop_result;
        return Err(error);
    }

    #[cfg(target_os = "ios")]
    stop_result?;
    Ok(())
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

    let retranscribe_started = std::time::Instant::now();
    let (text, asr_call_label) = coord.retranscribe_pcm(pcm).await?;
    if text.trim().is_empty() {
        return Err("重新转录仍未识别到语音".into());
    }
    let retranscribe_ms = retranscribe_started.elapsed().as_millis() as u64;

    // 找到原条目，保留其它字段，只更新转写结果 + 清错误码。
    let mut entry = coord
        .history()
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "history entry not found".to_string())?;
    apply_retranscription(&mut entry, text, &asr_call_label, retranscribe_ms);

    let updated = coord
        .history()
        .update_entry(entry.clone())
        .map_err(|e| e.to_string())?;
    if !updated {
        return Err("history entry not found".into());
    }
    Ok(entry)
}


/// 把一次重转录的结果落到既有历史条目上（纯函数，供单测覆盖契约）：
/// - 只更新转写结果并清除失败标记。insert_status 保持原值——重新转录不向光标落字，
///   没有可表达「已转写未落字」的状态，清掉 error_code 即足以标记不再是失败条目。
/// - ASR 归因换成本次重转实际构建的 (provider, model) 快照 + 实测耗时。
/// - 重转没有润色环节：清掉 llm_* / polish_ms，避免详情页把旧润色信息错挂在新转写上。
fn apply_retranscription(
    entry: &mut DictationSession,
    text: String,
    asr_call_label: &crate::coordinator::AsrCallLabel,
    asr_ms: u64,
) {
    entry.raw_transcript = text.clone();
    entry.final_text = text;
    entry.error_code = None;
    entry.asr_provider = Some(asr_call_label.provider.clone());
    entry.asr_model = asr_call_label.model.clone();
    entry.asr_ms = Some(asr_ms);
    entry.llm_provider = None;
    entry.llm_model = None;
    entry.polish_ms = None;
}

#[cfg(test)]
mod retranscribe_tests {
    use super::apply_retranscription;
    use crate::coordinator::AsrCallLabel;
    use crate::types::{DictationSession, HistorySource, InsertStatus, PolishMode};

    fn failed_entry() -> DictationSession {
        DictationSession {
            id: "s1".into(),
            created_at: "2026-07-15T00:00:00Z".into(),
            source: HistorySource::Voice,
            raw_transcript: String::new(),
            final_text: String::new(),
            mode: PolishMode::Light,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: None,
            insert_status: InsertStatus::Failed,
            error_code: Some("transcribeFailed".into()),
            duration_ms: Some(3200),
            dictionary_entry_count: None,
            has_audio_recording: Some(true),
            asr_provider: Some("volcengine".into()),
            asr_model: Some("volc.seedasr.sauc.duration".into()),
            llm_provider: Some("ark".into()),
            llm_model: Some("deepseek-v3-2".into()),
            asr_ms: Some(15000),
            polish_ms: Some(1200),
        }
    }

    #[test]
    fn retranscription_overwrites_asr_attribution_and_clears_polish_fields() {
        let mut entry = failed_entry();
        let label = AsrCallLabel {
            provider: "bailian-qwen3-realtime".into(),
            model: Some("qwen3-asr-flash-realtime".into()),
        };
        apply_retranscription(&mut entry, "重转出来的文本".into(), &label, 480);

        assert_eq!(entry.raw_transcript, "重转出来的文本");
        assert_eq!(entry.final_text, "重转出来的文本");
        assert_eq!(entry.error_code, None, "重转成功应清除失败标记");
        // ASR 归因换成本次重转的构建时快照。
        assert_eq!(entry.asr_provider.as_deref(), Some("bailian-qwen3-realtime"));
        assert_eq!(entry.asr_model.as_deref(), Some("qwen3-asr-flash-realtime"));
        assert_eq!(entry.asr_ms, Some(480));
        // 重转没有润色环节：旧 LLM 元数据不得残留在新转写结果上。
        assert_eq!(entry.llm_provider, None);
        assert_eq!(entry.llm_model, None);
        assert_eq!(entry.polish_ms, None);
        // 其余字段保持原值。
        assert_eq!(entry.insert_status, InsertStatus::Failed);
        assert_eq!(entry.duration_ms, Some(3200));
        assert_eq!(entry.has_audio_recording, Some(true));
    }
}
