#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! 本地 Qwen3-ASR 一键"加载 + 测试"实现。
//!
//! 流程：
//!   1. 用 antirez 项目自带的 `samples/test_speech.wav` 作输入（编进二进制）
//!   2. WAV 解析（16kHz mono 16-bit PCM，但 fmt 后面可能有 LIST/INFO 等
//!      非 data chunk，必须按 RIFF 标准走 chunk 链找 "data"，不能 +44 硬偏移）
//!   3. 加载模型，跑 batch transcribe，分别记录 load_ms / transcribe_ms
//!   4. 给前端用：用户点击「加载并测试」按钮立即知道模型是否能跑、有多快、识别什么

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::Arc;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::time::Instant;

use anyhow::Result;
use serde::Serialize;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::models::model_dir;
use super::models::ModelId;

/// 内嵌测试音频。原始文件 `vendor/qwen-asr/samples/test_speech.wav`
/// 内容："Hello. This is a test of the Voxtrail speech-to-text system."
#[cfg(any(target_os = "macos", target_os = "linux"))]
const TEST_WAV: &[u8] = include_bytes!("../../../vendor/qwen-asr/samples/test_speech.wav");

/// 测试结果给前端展示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub backend: String,
    pub model_id: String,
    pub expected_text: String,
    pub transcribed_text: String,
    pub audio_ms: u64,
    pub load_ms: u64,
    pub transcribe_ms: u64,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub async fn run_test(
    model_id: ModelId,
    backend: Option<super::QwenBackend>,
) -> Result<TestResult> {
    if model_id.is_whisper() {
        #[cfg(target_os = "macos")]
        return run_whisper_test(model_id).await;
        #[cfg(target_os = "linux")]
        anyhow::bail!("本地 Whisper 测试仅支持 macOS");
    }
    let backend =
        backend.ok_or_else(|| anyhow::anyhow!("当前系统不支持所选的本地 Qwen3-ASR 后端"))?;
    let dir = model_dir(model_id)?;
    if !dir.exists() {
        anyhow::bail!("模型目录不存在：{}（请先下载）", dir.display());
    }

    // ── 模型文件完整性检查 ────────────────────────────────────────────
    // 在调 native 引擎之前先检查关键文件是否齐全、尺寸是否合理，避免因下载不完整
    // 或文件损坏导致模型加载失败。tokenizer.json 会在 MLX 引擎首次加载时从
    // vocab.json / merges.txt 本地生成。
    let required_files = ["config.json", "vocab.json", "merges.txt"];
    for fname in &required_files {
        let path = dir.join(fname);
        if !path.exists() {
            anyhow::bail!(
                "模型文件缺失：{fname}，请重新下载（预期路径：{}）",
                path.display()
            );
        }
        let meta = std::fs::metadata(&path)
            .map_err(|e| anyhow::anyhow!("读取 {fname} 元数据失败：{e}"))?;
        if meta.len() == 0 {
            anyhow::bail!("模型文件为空：{fname}，请重新下载");
        }
    }
    // safetensors 可能是单文件 model.safetensors 或分片 model-00001-of-NNNN.safetensors
    let has_safetensors: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| anyhow::anyhow!("读取模型目录失败：{e}"))?
        .filter_map(|entry| entry.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "safetensors"))
        .collect();
    if has_safetensors.is_empty() {
        anyhow::bail!("模型目录中没有 .safetensors 权重文件，请重新下载");
    }
    for entry in &has_safetensors {
        let meta = std::fs::metadata(entry.path())
            .map_err(|e| anyhow::anyhow!("读取 {} 元数据失败：{e}", entry.path().display()))?;
        if meta.len() < 1024 {
            anyhow::bail!(
                "权重文件太小（{} bytes）：{}，请重新下载",
                meta.len(),
                entry.path().display()
            );
        }
    }

    let samples = decode_wav_16k_mono(TEST_WAV)?;
    let audio_ms = (samples.len() as u64) * 1000 / 16_000;

    // 本地模型加载是同步阻塞调用且较慢（数秒）；扔到 spawn_blocking 不阻塞 tokio runtime。
    let load_start = Instant::now();
    let dir_for_blocking = dir.clone();
    let engine =
        tauri::async_runtime::spawn_blocking(move || load_engine(backend, &dir_for_blocking))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??;
    let load_ms = load_start.elapsed().as_millis() as u64;

    // batch transcribe 也是阻塞 + 重活，同样扔到 blocking pool。
    let trans_start = Instant::now();
    let engine_clone = Arc::clone(&engine);
    let transcribe =
        tauri::async_runtime::spawn_blocking(move || engine_clone.transcribe_pcm(&samples));
    let text = match tokio::time::timeout(std::time::Duration::from_secs(30), transcribe).await {
        Ok(joined) => {
            joined.map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??
        }
        Err(_) => {
            engine.cancel();
            anyhow::bail!("本地 Qwen3-ASR 加载并测试转写超时（30 秒）");
        }
    };
    let transcribe_ms = trans_start.elapsed().as_millis() as u64;

    Ok(TestResult {
        backend: match backend {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            super::QwenBackend::Mlx => "MLX Metal (Apple Silicon)",
            super::QwenBackend::C => "C CPU",
        }
        .into(),
        model_id: model_id.as_str().into(),
        expected_text: "Hello. This is a test of the Voxtrail speech-to-text system.".into(),
        transcribed_text: text,
        audio_ms,
        load_ms,
        transcribe_ms,
    })
}

#[cfg(target_os = "macos")]
async fn run_whisper_test(model_id: ModelId) -> Result<TestResult> {
    use super::whisper_provider::{LocalWhisperCache, WhisperEngine};

    let path = super::whisper_provider::model_path_for_model(model_id.as_str())?;
    if !path.is_file() {
        anyhow::bail!("模型文件不存在：{}（请先下载）", path.display());
    }
    let samples = decode_wav_16k_mono(TEST_WAV)?;
    let audio_ms = samples.len() as u64 * 1000 / 16_000;
    let cache = LocalWhisperCache::new();
    let load_start = Instant::now();
    let path_for_blocking = path.clone();
    let model_name = model_id.as_str().to_string();
    let engine = tauri::async_runtime::spawn_blocking(move || {
        cache.get_or_load(&model_name, &path_for_blocking)
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??;
    let load_ms = load_start.elapsed().as_millis() as u64;

    let trans_start = Instant::now();
    let text = tauri::async_runtime::spawn_blocking(move || {
        WhisperEngine::transcribe(&engine, &samples, "en")
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??;
    let transcribe_ms = trans_start.elapsed().as_millis() as u64;

    Ok(TestResult {
        backend: "whisper.cpp (Metal/CPU)".into(),
        model_id: model_id.as_str().into(),
        expected_text: "Hello. This is a test of the Voxtrail speech-to-text system.".into(),
        transcribed_text: text,
        audio_ms,
        load_ms,
        transcribe_ms,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub async fn run_test(
    _model_id: ModelId,
    _backend: Option<super::QwenBackend>,
) -> Result<TestResult> {
    anyhow::bail!("本地 Qwen3-ASR C 后端目前仅支持 macOS/Linux；MLX 后端仅支持 macOS")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn load_engine(backend: super::QwenBackend, dir: &Path) -> Result<Arc<super::LocalQwenEngine>> {
    let engine = super::LocalQwenEngine::load(backend, dir)?;
    Ok(Arc::new(engine))
}

/// 严格按 RIFF 走 chunk 链找 "data" —— jfk.wav / test_speech.wav 都在
/// fmt chunk 后面带了 LIST/INFO 元数据，硬编码 +44 会读到垃圾。
fn decode_wav_16k_mono(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        anyhow::bail!("不是有效的 RIFF/WAVE 文件");
    }

    let mut cursor = 12usize;
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;
    let mut bits_per_sample: u16 = 0;
    let mut data_offset: usize = 0;
    let mut data_size: usize = 0;

    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let body_start = cursor + 8;

        match id {
            b"fmt " => {
                if body_start + 16 > bytes.len() {
                    anyhow::bail!("fmt chunk 越界");
                }
                let format =
                    u16::from_le_bytes(bytes[body_start..body_start + 2].try_into().unwrap());
                if format != 1 {
                    anyhow::bail!("只支持 PCM（format=1），当前 format={format}");
                }
                channels =
                    u16::from_le_bytes(bytes[body_start + 2..body_start + 4].try_into().unwrap());
                sample_rate =
                    u32::from_le_bytes(bytes[body_start + 4..body_start + 8].try_into().unwrap());
                bits_per_sample =
                    u16::from_le_bytes(bytes[body_start + 14..body_start + 16].try_into().unwrap());
            }
            b"data" => {
                data_offset = body_start;
                data_size = size;
                break;
            }
            _ => { /* LIST / INFO / 其它 metadata —— 跳过 */ }
        }
        // chunk 体长度需按偶数对齐
        let advance = size + (size & 1);
        cursor = body_start + advance;
    }

    if data_offset == 0 || data_size == 0 {
        anyhow::bail!("未找到 data chunk");
    }
    if sample_rate != 16_000 || channels != 1 || bits_per_sample != 16 {
        anyhow::bail!(
            "测试 WAV 必须是 16kHz mono 16-bit；实际 {sample_rate}Hz / {channels}ch / {bits_per_sample}bit"
        );
    }

    let data_end = (data_offset + data_size).min(bytes.len());
    let samples_i16 = &bytes[data_offset..data_end];
    let samples: Vec<f32> = samples_i16
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect();
    Ok(samples)
}
