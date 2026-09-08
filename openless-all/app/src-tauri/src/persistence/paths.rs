#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Storage path resolution: models root, recordings archive (with retention
//! pruning), and the Windows
//! Foundry Local cache roots.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{data_dir, ensure_dir, HISTORY_CAP, PREFERENCES_FILE};

/// 默认模型根目录：`<data_dir>/models/`。
pub fn default_models_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("models");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// 把用户选择的父目录转成实际模型根目录。
///
/// UI 让用户选一个普通目录；OpenLess 固定在其下创建 `OpenLess/models/`，
/// 避免把多个引擎的模型文件直接散落在用户选择目录根部。
pub fn models_root_for_base_dir(base_dir: Option<&str>) -> Result<PathBuf> {
    let trimmed = base_dir.map(str::trim).filter(|value| !value.is_empty());
    let dir = match trimmed {
        Some(base) => PathBuf::from(base).join("OpenLess").join("models"),
        None => return default_models_root(),
    };
    ensure_dir(&dir)?;
    Ok(dir)
}

fn configured_models_base_dir() -> Result<Option<String>> {
    let path = data_dir()?.join(PREFERENCES_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let value = serde_json::from_slice::<serde_json::Value>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))?;
    Ok(value
        .get("localAsrModelsBaseDir")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

/// 当前配置下的实际模型根目录。
pub fn models_root() -> Result<PathBuf> {
    models_root_for_base_dir(configured_models_base_dir()?.as_deref())
}

/// 录音归档目录：`<data_dir>/recordings/`。
/// 仅当用户开 `prefs.record_audio_for_debug` 时才会有内容（每次会话一个 `<session_id>.wav`）。
/// 同样受 `history_retention_days` 清理（写入新文件时顺手裁旧的）。
pub fn recordings_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("recordings");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// 双重 cap 清理 `recordings/*.wav`：
/// - `retention_days > 0` → 把超过 N 天的删掉（沿用 history 的 retention 逻辑）。
/// - `max_entries == Some(n)` → 按 mtime 倒序保留最新的 n 条（clamp 到 1..=HISTORY_CAP）；
///   `None` 时退回 HISTORY_CAP (200) 硬上限，避免无限增长。
/// 调用方：每次新建一条录音前。失败仅打 warn，避免影响主路径。
pub fn prune_recordings(retention_days: u32, max_entries: Option<u32>) -> Result<()> {
    let dir = match data_dir() {
        Ok(d) => d.join("recordings"),
        Err(_) => return Ok(()),
    };
    if !dir.exists() {
        return Ok(());
    }

    // 第一步：按天清理。仅扫 .wav，跟第二步保持一致；metadata 读不到的文件按"过期"处理
    // —— fs 损坏 / 未来格式不一致的孤儿文件应当被回收而不是无限累积。
    if retention_days > 0 {
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(u64::from(retention_days) * 24 * 3600);
        for entry in fs::read_dir(&dir).context("read recordings dir")?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
                continue;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            if modified < cutoff {
                if let Err(err) = fs::remove_file(&path) {
                    log::warn!("[recordings] prune (days) remove failed for {path:?}: {err}");
                }
            }
        }
    }

    // 第二步：按条数清理。剩下的 wav 按 mtime 倒序，超出 cap 的删掉。
    let cap = max_entries
        .map(|n| (n as usize).clamp(1, HISTORY_CAP))
        .unwrap_or(HISTORY_CAP);
    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(&dir)
        .context("read recordings dir")?
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            // 只看 .wav，避免误删未来其他类型的归档文件。
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect();
    if entries.len() <= cap {
        return Ok(());
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in entries.into_iter().skip(cap) {
        if let Err(err) = fs::remove_file(&path) {
            log::warn!(
                "[recordings] prune (count) remove failed for {:?}: {err}",
                path
            );
        }
    }
    Ok(())
}

/// 单个 session 的录音文件路径。不保证文件已存在（DictationSession.has_audio_recording
/// 决定文件是否被写过）。前端用 `read_audio_recording` IPC 读字节流喂 HTMLAudio。
pub fn recording_path_for_session(session_id: &str) -> Result<PathBuf> {
    Ok(recordings_root()?.join(format!("{session_id}.wav")))
}

/// Foundry Local 下载与缓存根目录。DLL 和模型都不打进安装包，和 Qwen3-ASR
/// 一样放在 OpenLess 的 models 目录下，卸载清理用户数据时可以一起删除。
#[cfg(target_os = "windows")]
pub fn foundry_local_root() -> Result<PathBuf> {
    let dir = models_root()?.join("foundry-local");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_native_runtime_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("runtime");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_model_cache_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?;
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_app_data_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("app-data");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_logs_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("logs");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::models_root_for_base_dir;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn custom_models_root_uses_openless_models_suffix() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-model-root-{}", uuid::Uuid::new_v4()));
        let root = models_root_for_base_dir(Some(tmp.to_string_lossy().as_ref()))
            .expect("build custom models root");

        assert_eq!(root, tmp.join("OpenLess").join("models"));
        assert!(root.is_dir());

        let _ = fs::remove_dir_all(&tmp);
    }
}
