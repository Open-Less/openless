#![allow(dead_code, unused_imports, unused_variables)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use super::download::{
    build_client, download_one, partial_actual_size, DownloadPhase, DownloadProgress, Mirror,
};
use super::sherpa;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SherpaRemoteFile {
    pub path: String,
    pub local_path: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SherpaRemoteInfo {
    pub model_alias: String,
    pub mirror: String,
    pub files: Vec<SherpaRemoteFile>,
    pub total_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct HfTreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    path: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    lfs: Option<HfLfsInfo>,
}

#[derive(Debug, Deserialize)]
struct HfLfsInfo {
    oid: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Default)]
pub struct SherpaDownloadManager {
    cancel_flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl SherpaDownloadManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(self: &Arc<Self>, app: AppHandle, model_alias: String, mirror: Mirror) {
        let key = model_alias.clone();
        let flag = {
            let mut flags = self.cancel_flags.lock();
            if flags.contains_key(&key) {
                log::info!("[sherpa-asr] 模型下载已在进行中: {key}");
                return;
            }
            let f = Arc::new(AtomicBool::new(false));
            flags.insert(key.clone(), Arc::clone(&f));
            f
        };

        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let result = run_download(&app, &model_alias, mirror, Arc::clone(&flag)).await;
            manager.cancel_flags.lock().remove(&key);
            match result {
                Ok(()) => log::info!("[sherpa-asr] 模型下载完成: {key}"),
                Err(error) => log::error!("[sherpa-asr] 模型下载失败: {key}: {error:#}"),
            }
        });
    }

    pub fn cancel(&self, model_alias: &str) {
        if let Some(flag) = self.cancel_flags.lock().get(model_alias) {
            flag.store(true, Ordering::SeqCst);
            log::info!("[sherpa-asr] 已请求取消模型下载: {model_alias}");
        } else {
            log::info!("[sherpa-asr] 请求取消模型下载，但没有活跃任务: {model_alias}");
        }
    }

    pub fn is_active(&self, model_alias: &str) -> bool {
        self.cancel_flags.lock().contains_key(model_alias)
    }
}

pub async fn fetch_remote_info(model_alias: &str, mirror: Mirror) -> Result<SherpaRemoteInfo> {
    if let Some(archive) = sherpa::release_archive_for_alias(model_alias) {
        return fetch_release_archive_info(model_alias, archive).await;
    }
    let client = build_client()?;
    let repo = sherpa::hf_repo_for_alias(model_alias)?;
    let url = format!("{}/api/models/{}/tree/main", mirror.base_url(), repo);
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("HF tree API GET 失败: {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("HF tree API HTTP {}: {url}", resp.status());
    }
    let entries: Vec<HfTreeEntry> = resp
        .json()
        .await
        .with_context(|| format!("HF tree JSON 解码失败: {url}"))?;

    let mut files = Vec::new();
    for (remote_path, local_path) in sherpa::download_files_for_alias(model_alias)? {
        let entry = entries
            .iter()
            .find(|entry| entry.entry_type == "file" && entry.path == *remote_path)
            .with_context(|| format!("Sherpa 模型文件清单缺少: {remote_path}"))?;
        let size = entry
            .lfs
            .as_ref()
            .and_then(|lfs| lfs.size)
            .or(entry.size)
            .unwrap_or(0);
        let sha256 = entry
            .lfs
            .as_ref()
            .map(|lfs| lfs.oid.clone())
            .filter(|oid| is_sha256_hex(oid));
        files.push(SherpaRemoteFile {
            path: (*remote_path).to_string(),
            local_path: (*local_path).to_string(),
            size,
            sha256,
        });
    }

    let total_bytes = files.iter().map(|file| file.size).sum();
    Ok(SherpaRemoteInfo {
        model_alias: model_alias.to_string(),
        mirror: mirror.as_str().to_string(),
        files,
        total_bytes,
    })
}

async fn fetch_release_archive_info(
    model_alias: &str,
    archive: sherpa::SherpaReleaseArchive,
) -> Result<SherpaRemoteInfo> {
    let client = build_client()?;
    let (size, sha256) = match fetch_release_archive_asset_info(&client, archive).await {
        Ok(info) => info,
        Err(error) => {
            log::warn!("[sherpa-asr] GitHub release API 获取包大小失败，回退 HEAD: {error:#}");
            let resp = client
                .head(archive.url)
                .send()
                .await
                .with_context(|| format!("GitHub release HEAD 失败: {}", archive.url))?;
            if !resp.status().is_success() {
                anyhow::bail!("GitHub release HTTP {}: {}", resp.status(), archive.url);
            }
            (resp.content_length().unwrap_or(0), None)
        }
    };
    Ok(SherpaRemoteInfo {
        model_alias: model_alias.to_string(),
        mirror: "github-release".to_string(),
        files: vec![SherpaRemoteFile {
            path: archive.file_name.to_string(),
            local_path: archive.file_name.to_string(),
            size,
            sha256,
        }],
        total_bytes: size,
    })
}

async fn fetch_release_archive_asset_info(
    client: &reqwest::Client,
    archive: sherpa::SherpaReleaseArchive,
) -> Result<(u64, Option<String>)> {
    let url = "https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/asr-models";
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GitHub release API GET 失败: {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("GitHub release API HTTP {}: {url}", resp.status());
    }
    let release: GithubRelease = resp
        .json()
        .await
        .with_context(|| format!("GitHub release API JSON 解码失败: {url}"))?;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == archive.file_name)
        .with_context(|| format!("GitHub release asset 缺少: {}", archive.file_name))?;
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|digest| is_sha256_hex(digest))
        .map(str::to_string);
    Ok((asset.size, sha256))
}

pub fn downloaded_bytes(model_alias: &str) -> u64 {
    let Ok(dir) = sherpa::model_dir_for_alias(model_alias) else {
        return 0;
    };
    if let Some(archive) = sherpa::release_archive_for_alias(model_alias) {
        return downloaded_release_archive_bytes(&dir, model_alias, archive);
    }
    let Ok(files) = sherpa::download_files_for_alias(model_alias) else {
        return 0;
    };
    files
        .iter()
        .map(|(_, local_path)| {
            let dest = dir.join(local_path);
            if let Ok(meta) = std::fs::metadata(&dest) {
                meta.len()
            } else {
                partial_actual_size(&dest.with_extension("partial"))
            }
        })
        .sum()
}

fn downloaded_release_archive_bytes(
    dir: &Path,
    model_alias: &str,
    archive: sherpa::SherpaReleaseArchive,
) -> u64 {
    let dest = dir.join(archive.file_name);
    let (extracted, extracted_complete) = extracted_release_archive_bytes(dir, model_alias);
    if extracted_complete {
        return extracted;
    }
    if let Ok(meta) = std::fs::metadata(&dest) {
        return meta.len();
    }
    let partial = partial_actual_size(&dest.with_extension("partial"));
    partial.max(extracted)
}

fn finished_release_archive_progress_bytes(
    dir: &Path,
    model_alias: &str,
    archive: sherpa::SherpaReleaseArchive,
) -> (u64, u64) {
    let finished_bytes = downloaded_release_archive_bytes(dir, model_alias, archive);
    (finished_bytes, finished_bytes)
}

fn extracted_release_archive_bytes(dir: &Path, model_alias: &str) -> (u64, bool) {
    if let Ok(files) = sherpa::required_files_for_alias(model_alias) {
        let mut total = 0;
        let mut complete = true;
        for file in files {
            let path = dir.join(file);
            total += path_size_recursive(&path);
            if !sherpa::required_path_is_valid(model_alias, file, &path) {
                complete = false;
            }
        }
        return (total, complete);
    }
    (0, false)
}

fn path_size_recursive(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(meta) if meta.is_dir() => {
            let mut total: u64 = 0;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    total += path_size_recursive(&entry.path());
                }
            }
            total
        }
        _ => 0,
    }
}

async fn run_download(
    app: &AppHandle,
    model_alias: &str,
    mirror: Mirror,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let dir = sherpa::model_dir_for_alias(model_alias)?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create sherpa model dir failed: {}", dir.display()))?;
    if let Some(archive) = sherpa::release_archive_for_alias(model_alias) {
        return run_release_archive_download(app, model_alias, archive, &dir, cancel).await;
    }

    let client = build_client()?;
    let info = match fetch_remote_info(model_alias, mirror).await {
        Ok(info) => info,
        Err(error) => {
            emit(
                app,
                DownloadProgress {
                    model_id: model_alias.to_string(),
                    file: String::new(),
                    file_index: 0,
                    file_count: 0,
                    bytes_downloaded: 0,
                    bytes_total: 0,
                    phase: DownloadPhase::Failed,
                    error: Some(format!("拉文件清单失败: {error:#}")),
                },
            );
            return Err(error);
        }
    };
    let repo = sherpa::hf_repo_for_alias(model_alias)?;
    let total_bytes = info.total_bytes;
    let file_count = info.files.len();

    emit(
        app,
        DownloadProgress {
            model_id: model_alias.to_string(),
            file: String::new(),
            file_index: 0,
            file_count,
            bytes_downloaded: downloaded_bytes(model_alias),
            bytes_total: total_bytes,
            phase: DownloadPhase::Started,
            error: None,
        },
    );

    for file in &info.files {
        if let Some(parent) = dir.join(&file.local_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    let in_flight_bytes: Arc<Vec<AtomicU64>> =
        Arc::new(info.files.iter().map(|_| AtomicU64::new(0)).collect());
    let already_done_bytes: u64 = info
        .files
        .iter()
        .map(|file| {
            let dest = dir.join(&file.local_path);
            if file_is_verified(&dest, file) {
                file.size
            } else {
                0
            }
        })
        .sum();

    let semaphore = Arc::new(tokio::sync::Semaphore::new(3));
    let mut futs = futures_util::stream::FuturesUnordered::new();

    for (idx, file) in info.files.iter().cloned().enumerate() {
        let dest = dir.join(&file.local_path);
        if file_is_verified(&dest, &file) {
            continue;
        }
        if dest.exists() {
            let _ = std::fs::remove_file(&dest);
        }
        let url = format!("{}/{}/resolve/main/{}", mirror.base_url(), repo, file.path);
        let semaphore = Arc::clone(&semaphore);
        let client = client.clone();
        let cancel = Arc::clone(&cancel);
        let app = app.clone();
        let in_flight_bytes = Arc::clone(&in_flight_bytes);
        let model_alias_emit = model_alias.to_string();
        let file_path_emit = file.local_path.clone();
        let file_size = file.size;
        let total_bytes_cap = total_bytes;
        let already_done = already_done_bytes;

        futs.push(tauri::async_runtime::spawn(async move {
            let _permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return Err(anyhow::anyhow!("semaphore closed")),
            };
            if cancel.load(Ordering::SeqCst) {
                return Ok(());
            }
            let app_emit = app.clone();
            let in_flight_for_cb = Arc::clone(&in_flight_bytes);
            let on_progress: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |bytes_in_file| {
                in_flight_for_cb[idx].store(bytes_in_file, Ordering::Relaxed);
                let total_in_flight: u64 = in_flight_for_cb
                    .iter()
                    .map(|bytes| bytes.load(Ordering::Relaxed))
                    .sum();
                let _ = app_emit.emit(
                    "sherpa-onnx-asr-download-progress",
                    DownloadProgress {
                        model_id: model_alias_emit.clone(),
                        file: file_path_emit.clone(),
                        file_index: idx,
                        file_count,
                        bytes_downloaded: already_done + total_in_flight,
                        bytes_total: total_bytes_cap,
                        phase: DownloadPhase::Progress,
                        error: None,
                    },
                );
            });

            let result = download_one(
                &client,
                &url,
                &dest,
                file_size,
                Arc::clone(&cancel),
                on_progress,
            )
            .await;
            if result.is_ok() {
                verify_file(&dest, &file)?;
                in_flight_bytes[idx].store(file_size, Ordering::Relaxed);
            }
            result.with_context(|| format!("file {}", file.local_path))
        }));
    }

    let mut first_err: Option<anyhow::Error> = None;
    let mut self_aborted = false;
    while let Some(joined) = futs.next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                if first_err.is_none() {
                    first_err = Some(error);
                }
                if !cancel.load(Ordering::SeqCst) {
                    log::warn!("[sherpa-asr] 单文件下载失败，正在中止其它任务");
                    cancel.store(true, Ordering::SeqCst);
                    self_aborted = true;
                }
            }
            Err(error) => {
                if first_err.is_none() {
                    first_err = Some(anyhow::anyhow!("join: {error}"));
                }
            }
        }
    }

    if cancel.load(Ordering::SeqCst) && !self_aborted {
        emit_cancelled(app, model_alias, file_count, total_bytes);
        return Ok(());
    }
    if let Some(error) = first_err {
        emit_failed(app, model_alias, file_count, total_bytes, &error);
        return Err(error);
    }

    for file in &info.files {
        verify_file(&dir.join(&file.local_path), file)?;
    }

    emit(
        app,
        DownloadProgress {
            model_id: model_alias.to_string(),
            file: String::new(),
            file_index: file_count,
            file_count,
            bytes_downloaded: downloaded_bytes(model_alias),
            bytes_total: total_bytes,
            phase: DownloadPhase::Finished,
            error: None,
        },
    );
    Ok(())
}

async fn run_release_archive_download(
    app: &AppHandle,
    model_alias: &str,
    archive: sherpa::SherpaReleaseArchive,
    dir: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let client = build_client()?;
    let info = match fetch_release_archive_info(model_alias, archive).await {
        Ok(info) => info,
        Err(error) => {
            emit(
                app,
                DownloadProgress {
                    model_id: model_alias.to_string(),
                    file: String::new(),
                    file_index: 0,
                    file_count: 0,
                    bytes_downloaded: 0,
                    bytes_total: 0,
                    phase: DownloadPhase::Failed,
                    error: Some(format!("拉 release 包信息失败: {error:#}")),
                },
            );
            return Err(error);
        }
    };
    let total_bytes = info.total_bytes;
    let file_count = info.files.len();
    emit(
        app,
        DownloadProgress {
            model_id: model_alias.to_string(),
            file: String::new(),
            file_index: 0,
            file_count,
            bytes_downloaded: downloaded_bytes(model_alias),
            bytes_total: total_bytes,
            phase: DownloadPhase::Started,
            error: None,
        },
    );
    let archive_path = dir.join(archive.file_name);
    let app_emit = app.clone();
    let model_alias_emit = model_alias.to_string();
    let file_name_emit = archive.file_name.to_string();
    let on_progress: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |bytes_downloaded| {
        let _ = app_emit.emit(
            "sherpa-onnx-asr-download-progress",
            DownloadProgress {
                model_id: model_alias_emit.clone(),
                file: file_name_emit.clone(),
                file_index: 0,
                file_count,
                bytes_downloaded,
                bytes_total: total_bytes,
                phase: DownloadPhase::Progress,
                error: None,
            },
        );
    });
    let archive_file = info
        .files
        .first()
        .ok_or_else(|| anyhow::anyhow!("release archive file info missing"))?;
    let result = if archive_file_is_verified(&archive_path, archive_file) {
        Ok(())
    } else {
        if archive_path.exists() {
            remove_path_if_exists(&archive_path)?;
        }
        download_one(
            &client,
            archive.url,
            &archive_path,
            total_bytes,
            Arc::clone(&cancel),
            on_progress,
        )
        .await
    };
    if cancel.load(Ordering::SeqCst) {
        emit_cancelled(app, model_alias, file_count, total_bytes);
        return Ok(());
    }
    if let Err(error) = result {
        emit_failed(app, model_alias, file_count, total_bytes, &error);
        return Err(error);
    }
    if let Err(error) = verify_file(&archive_path, archive_file) {
        emit_failed(app, model_alias, file_count, total_bytes, &error);
        return Err(error);
    }
    let archive_path_for_extract = archive_path.clone();
    let dir_for_extract = dir.to_path_buf();
    let model_alias_for_extract = model_alias.to_string();
    let extract_result = tauri::async_runtime::spawn_blocking(move || {
        extract_release_archive(
            &archive_path_for_extract,
            &dir_for_extract,
            archive,
            &model_alias_for_extract,
        )
    })
    .await
    .map_err(|error| anyhow::anyhow!("extract join failed: {error:#}"))
    .and_then(|result| result);
    if let Err(error) = extract_result {
        emit_failed(app, model_alias, file_count, total_bytes, &error);
        return Err(error);
    }
    let (finished_bytes, finished_total_bytes) =
        finished_release_archive_progress_bytes(dir, model_alias, archive);
    emit(
        app,
        DownloadProgress {
            model_id: model_alias.to_string(),
            file: String::new(),
            file_index: file_count,
            file_count,
            bytes_downloaded: finished_bytes,
            bytes_total: finished_total_bytes,
            phase: DownloadPhase::Finished,
            error: None,
        },
    );
    Ok(())
}

fn extract_release_archive(
    archive_path: &Path,
    dir: &Path,
    archive: sherpa::SherpaReleaseArchive,
    model_alias: &str,
) -> Result<()> {
    let extract_dir = archive_extract_dir(dir)?;
    remove_path_if_exists(&extract_dir)?;
    std::fs::create_dir_all(&extract_dir)
        .with_context(|| format!("create extract dir failed: {}", extract_dir.display()))?;
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("open archive failed: {}", archive_path.display()))?;
    let decoder = bzip2::read::BzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    tar.unpack(&extract_dir)
        .with_context(|| format!("unpack archive failed: {}", archive_path.display()))?;
    let root = extract_dir.join(archive.root_dir);
    if !root.exists() {
        anyhow::bail!("archive root missing: {}", root.display());
    }
    let required_files = sherpa::required_files_for_alias(model_alias)?;
    for required in required_files {
        let src = root.join(required);
        if !sherpa::required_path_is_valid(model_alias, required, &src) {
            anyhow::bail!("archive required path missing: {}", src.display());
        }
    }
    for required in required_files {
        let src = root.join(required);
        let dest = dir.join(required);
        move_path(&src, &dest)?;
    }
    remove_path_if_exists(&extract_dir)?;
    let _ = std::fs::remove_file(archive_path);
    Ok(())
}

fn archive_extract_dir(dir: &Path) -> Result<PathBuf> {
    let name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid model dir: {}", dir.display()))?;
    Ok(dir.with_file_name(format!("{name}.extracting")))
}

fn move_path(src: &Path, dest: &Path) -> Result<()> {
    if !src.exists() {
        anyhow::bail!("archive required path missing: {}", src.display());
    }
    remove_path_if_exists(dest)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent dir failed: {}", parent.display()))?;
    }
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) if src.is_dir() => {
            copy_dir_recursive(src, dest)?;
            std::fs::remove_dir_all(src)
                .with_context(|| format!("remove moved dir failed: {}", src.display()))?;
            Ok(())
        }
        Err(_) => {
            std::fs::copy(src, dest).with_context(|| {
                format!("copy file failed: {} -> {}", src.display(), dest.display())
            })?;
            std::fs::remove_file(src)
                .with_context(|| format!("remove moved file failed: {}", src.display()))?;
            Ok(())
        }
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("create dir failed: {}", dest.display()))?;
    for entry in
        std::fs::read_dir(src).with_context(|| format!("read dir failed: {}", src.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path).with_context(|| {
                format!(
                    "copy file failed: {} -> {}",
                    src_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path)
            .with_context(|| format!("remove dir failed: {}", path.display())),
        Ok(_) => std::fs::remove_file(path)
            .with_context(|| format!("remove file failed: {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("stat failed: {}", path.display())),
    }
}

fn file_is_verified(path: &Path, file: &SherpaRemoteFile) -> bool {
    path.exists() && verify_file(path, file).is_ok()
}

fn archive_file_is_verified(path: &Path, file: &SherpaRemoteFile) -> bool {
    path.exists() && (file.size > 0 || file.sha256.is_some()) && verify_file(path, file).is_ok()
}

fn verify_file(path: &Path, file: &SherpaRemoteFile) -> Result<()> {
    let meta =
        std::fs::metadata(path).with_context(|| format!("stat failed: {}", path.display()))?;
    if file.size > 0 && meta.len() != file.size {
        anyhow::bail!(
            "文件大小不匹配: {} actual={} expected={}",
            path.display(),
            meta.len(),
            file.size
        );
    }
    if let Some(expected) = &file.sha256 {
        let actual = sha256_file(path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            anyhow::bail!(
                "SHA-256 不匹配: {} actual={} expected={}",
                path.display(),
                actual,
                expected
            );
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open for sha256 failed: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)
            .with_context(|| format!("read for sha256 failed: {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn emit(app: &AppHandle, payload: DownloadProgress) {
    if let Err(error) = app.emit("sherpa-onnx-asr-download-progress", payload) {
        log::warn!("[sherpa-asr] 发送下载进度失败: {error}");
    }
}

fn emit_cancelled(app: &AppHandle, model_alias: &str, file_count: usize, total_bytes: u64) {
    emit(
        app,
        DownloadProgress {
            model_id: model_alias.to_string(),
            file: String::new(),
            file_index: 0,
            file_count,
            bytes_downloaded: downloaded_bytes(model_alias),
            bytes_total: total_bytes,
            phase: DownloadPhase::Cancelled,
            error: None,
        },
    );
}

fn emit_failed(
    app: &AppHandle,
    model_alias: &str,
    file_count: usize,
    total_bytes: u64,
    error: &anyhow::Error,
) {
    emit(
        app,
        DownloadProgress {
            model_id: model_alias.to_string(),
            file: String::new(),
            file_index: 0,
            file_count,
            bytes_downloaded: downloaded_bytes(model_alias),
            bytes_total: total_bytes,
            phase: DownloadPhase::Failed,
            error: Some(format!("{error:#}")),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    struct TempModelDir(PathBuf);

    impl TempModelDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "openless-sherpa-download-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("create temp model dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempModelDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
            if let Ok(extract_dir) = archive_extract_dir(&self.0) {
                let _ = fs::remove_dir_all(extract_dir);
            }
        }
    }

    fn write_release_archive_fixture(
        archive_path: &Path,
        archive: sherpa::SherpaReleaseArchive,
        files: &[(&str, &[u8])],
    ) {
        let src_root = std::env::temp_dir().join(format!(
            "openless-sherpa-archive-src-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&src_root).expect("create archive source root");
        for (relative, bytes) in files {
            let path = src_root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create archive source parent");
            }
            fs::write(path, bytes).expect("write archive source file");
        }

        let file = fs::File::create(archive_path).expect("create archive file");
        let encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        builder
            .append_dir_all(archive.root_dir, &src_root)
            .expect("append archive root");
        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish bzip2");
        let _ = fs::remove_dir_all(src_root);
    }

    #[test]
    fn verify_file_rejects_size_mismatch() {
        let dir = TempModelDir::new("verify-size");
        let path = dir.path().join("model.bin");
        fs::write(&path, b"abc").expect("write test file");
        let file = SherpaRemoteFile {
            path: "model.bin".into(),
            local_path: "model.bin".into(),
            size: 4,
            sha256: None,
        };

        let message = format!("{:#}", verify_file(&path, &file).unwrap_err());

        assert!(message.contains("文件大小不匹配"));
        assert!(message.contains("actual=3"));
        assert!(message.contains("expected=4"));
    }

    #[test]
    fn verify_file_rejects_sha256_mismatch() {
        let dir = TempModelDir::new("verify-sha");
        let path = dir.path().join("model.bin");
        fs::write(&path, b"abc").expect("write test file");
        let file = SherpaRemoteFile {
            path: "model.bin".into(),
            local_path: "model.bin".into(),
            size: 3,
            sha256: Some("0000000000000000000000000000000000000000000000000000000000000000".into()),
        };

        let message = format!("{:#}", verify_file(&path, &file).unwrap_err());

        assert!(message.contains("SHA-256 不匹配"));
        assert!(message
            .contains("expected=0000000000000000000000000000000000000000000000000000000000000000"));
    }

    #[test]
    fn verify_file_accepts_case_insensitive_sha256() {
        let dir = TempModelDir::new("verify-sha-ok");
        let path = dir.path().join("model.bin");
        fs::write(&path, b"abc").expect("write test file");
        let file = SherpaRemoteFile {
            path: "model.bin".into(),
            local_path: "model.bin".into(),
            size: 3,
            sha256: Some(sha256_file(&path).unwrap().to_ascii_uppercase()),
        };

        verify_file(&path, &file).expect("sha should verify");
    }

    #[test]
    fn archive_extract_dir_uses_sibling_path() {
        let dir = TempModelDir::new("extract-dir");
        let name = dir.path().file_name().unwrap().to_string_lossy();

        let extract_dir = archive_extract_dir(dir.path()).unwrap();

        assert_eq!(
            extract_dir,
            dir.path().with_file_name(format!("{name}.extracting"))
        );
    }

    #[test]
    fn release_archive_bytes_uses_partial_archive_when_not_extracted() {
        let alias = "qwen3-asr-0.6b-int8";
        let archive = sherpa::release_archive_for_alias(alias).expect("release archive");
        let dir = TempModelDir::new("release-archive-partial");
        let partial_path = dir.path().join(archive.file_name).with_extension("partial");
        fs::write(partial_path, b"partial").expect("write partial archive");

        assert_eq!(
            downloaded_release_archive_bytes(dir.path(), alias, archive),
            7
        );
    }

    #[test]
    fn extract_release_archive_rejects_missing_required_file() {
        let alias = "qwen3-asr-0.6b-int8";
        let archive = sherpa::release_archive_for_alias(alias).expect("release archive");
        let dir = TempModelDir::new("release-archive-missing");
        let archive_path = dir.path().join(archive.file_name);
        write_release_archive_fixture(
            &archive_path,
            archive,
            &[("conv_frontend.onnx", b"conv" as &[u8])],
        );

        let message = format!(
            "{:#}",
            extract_release_archive(&archive_path, dir.path(), archive, alias).unwrap_err()
        );

        assert!(message.contains("archive required path missing"));
        assert!(message.contains("encoder.int8.onnx"));
    }

    #[test]
    fn extract_release_archive_moves_required_files_and_removes_work_paths() {
        let alias = "qwen3-asr-0.6b-int8";
        let archive = sherpa::release_archive_for_alias(alias).expect("release archive");
        let dir = TempModelDir::new("release-archive-success");
        let archive_path = dir.path().join(archive.file_name);
        write_release_archive_fixture(
            &archive_path,
            archive,
            &[
                ("conv_frontend.onnx", b"conv" as &[u8]),
                ("encoder.int8.onnx", b"encoder" as &[u8]),
                ("decoder.int8.onnx", b"decoder" as &[u8]),
                ("tokenizer/tokenizer.json", b"tok" as &[u8]),
            ],
        );

        extract_release_archive(&archive_path, dir.path(), archive, alias).unwrap();

        assert_eq!(
            fs::read(dir.path().join("conv_frontend.onnx")).unwrap(),
            b"conv"
        );
        assert_eq!(
            fs::read(dir.path().join("encoder.int8.onnx")).unwrap(),
            b"encoder"
        );
        assert_eq!(
            fs::read(dir.path().join("decoder.int8.onnx")).unwrap(),
            b"decoder"
        );
        assert_eq!(
            fs::read(dir.path().join("tokenizer").join("tokenizer.json")).unwrap(),
            b"tok"
        );
        assert!(!archive_path.exists());
        assert!(!archive_extract_dir(dir.path()).unwrap().exists());
    }

    #[test]
    fn download_manager_cancel_sets_active_flag() {
        let manager = SherpaDownloadManager::new();
        let flag = Arc::new(AtomicBool::new(false));
        manager
            .cancel_flags
            .lock()
            .insert("sense-voice-small-zh".into(), Arc::clone(&flag));

        manager.cancel("sense-voice-small-zh");

        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn release_archive_downloaded_bytes_uses_extracted_assets_after_archive_removed() {
        let alias = "qwen3-asr-0.6b-int8";
        let archive = sherpa::release_archive_for_alias(alias).expect("release archive");
        let dir = TempModelDir::new("release-archive-extracted");
        fs::write(dir.path().join("conv_frontend.onnx"), b"abc").expect("write conv frontend");
        fs::write(dir.path().join("encoder.int8.onnx"), b"encod").expect("write encoder");
        fs::write(dir.path().join("decoder.int8.onnx"), b"decoder").expect("write decoder");
        fs::create_dir_all(dir.path().join("tokenizer")).expect("create tokenizer dir");
        fs::write(dir.path().join("tokenizer").join("tokenizer.json"), b"tok")
            .expect("write tokenizer file");

        assert!(!dir.path().join(archive.file_name).exists());
        assert_eq!(
            downloaded_release_archive_bytes(dir.path(), alias, archive),
            18
        );
    }

    #[test]
    fn release_archive_finished_progress_uses_cached_bytes_as_total() {
        let alias = "qwen3-asr-0.6b-int8";
        let archive = sherpa::release_archive_for_alias(alias).expect("release archive");
        let dir = TempModelDir::new("release-archive-finished-progress");
        fs::write(dir.path().join("conv_frontend.onnx"), b"abc").expect("write conv frontend");
        fs::write(dir.path().join("encoder.int8.onnx"), b"encod").expect("write encoder");
        fs::write(dir.path().join("decoder.int8.onnx"), b"decoder").expect("write decoder");
        fs::create_dir_all(dir.path().join("tokenizer")).expect("create tokenizer dir");
        fs::write(dir.path().join("tokenizer").join("tokenizer.json"), b"tok")
            .expect("write tokenizer file");

        let (downloaded, total) =
            finished_release_archive_progress_bytes(dir.path(), alias, archive);

        assert_eq!(downloaded, 18);
        assert_eq!(total, downloaded);
    }
}
