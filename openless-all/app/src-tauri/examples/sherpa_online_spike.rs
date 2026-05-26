//! Manual probe for the Windows sherpa-onnx Zipformer streaming model.
//!
//! Run from `openless-all/app`:
//!
//! ```text
//! cargo run --manifest-path src-tauri/Cargo.toml --example sherpa_online_spike -- <model-dir> <audio.wav|audio.s16le> [chunk-ms]
//! ```
//!
//! This bypasses OpenLess UI/coordinator code and exercises the native
//! `OnlineRecognizer` directly, so it is useful for collecting partial/final
//! output, latency, and RTF before validating the full dictation path.

#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::time::Instant;

#[cfg(target_os = "windows")]
use anyhow::{Context, Result};
#[cfg(target_os = "windows")]
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, Wave};

#[cfg(target_os = "windows")]
#[no_mangle]
#[used]
pub static openless_common_controls_v6_manifest_dependency_anchor: i32 = 0;

#[cfg(target_os = "windows")]
const MODEL_REPO: &str = "csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20";

#[cfg(target_os = "windows")]
const REQUIRED_FILES: &[&str] = &[
    "encoder-epoch-99-avg-1.int8.onnx",
    "decoder-epoch-99-avg-1.onnx",
    "joiner-epoch-99-avg-1.int8.onnx",
    "tokens.txt",
];

#[cfg(target_os = "windows")]
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        print_usage();
        std::process::exit(2);
    }

    let model_dir = PathBuf::from(&args[1]);
    let audio_path = PathBuf::from(&args[2]);
    let chunk_ms = args
        .get(3)
        .map(|value| value.parse::<usize>())
        .transpose()
        .context("chunk-ms must be an integer")?
        .unwrap_or(320)
        .clamp(20, 2_000);

    ensure_required_files(&model_dir)?;
    let (samples, sample_rate) = read_audio(&audio_path)?;
    if sample_rate != 16_000 {
        anyhow::bail!("expected 16 kHz mono audio, got {sample_rate} Hz");
    }

    let audio_secs = samples.len() as f64 / sample_rate as f64;
    println!(
        "model_dir={} audio={} samples={} sample_rate={} chunk_ms={}",
        model_dir.display(),
        audio_path.display(),
        samples.len(),
        sample_rate,
        chunk_ms
    );

    let recognizer = create_recognizer(&model_dir)?;
    let stream = recognizer.create_stream();
    let chunk_samples = (sample_rate as usize * chunk_ms / 1_000).max(1);
    let mut last_partial = String::new();
    let mut committed = String::new();
    let started = Instant::now();

    for chunk in samples.chunks(chunk_samples) {
        stream.accept_waveform(sample_rate, chunk);
        while recognizer.is_ready(&stream) {
            recognizer.decode(&stream);
            capture_result(&recognizer, &stream, &mut last_partial, &mut committed);
            if recognizer.is_endpoint(&stream) {
                if !last_partial.is_empty() {
                    append_segment(&mut committed, &last_partial);
                    println!("final-segment: {}", last_partial);
                }
                last_partial.clear();
                recognizer.reset(&stream);
            }
        }
    }

    stream.input_finished();
    while recognizer.is_ready(&stream) {
        recognizer.decode(&stream);
        capture_result(&recognizer, &stream, &mut last_partial, &mut committed);
    }
    if !last_partial.is_empty() {
        append_segment(&mut committed, &last_partial);
    }

    let elapsed_secs = started.elapsed().as_secs_f64();
    println!("final: {}", committed.trim());
    println!(
        "stats: audio_secs={:.3} elapsed_secs={:.3} rtf={:.3}",
        audio_secs,
        elapsed_secs,
        elapsed_secs / audio_secs.max(0.001)
    );

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("sherpa_online_spike is only available on Windows");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn print_usage() {
    eprintln!(
        "usage: cargo run --manifest-path src-tauri/Cargo.toml --example sherpa_online_spike -- <model-dir> <audio.wav|audio.s16le> [chunk-ms]"
    );
    eprintln!("model: {MODEL_REPO}");
    eprintln!("audio: 16 kHz mono WAV, or raw s16le/pcm");
    eprintln!("chunk-ms: optional streaming chunk size, clamped to 20..2000 ms; default 320");
}

#[cfg(target_os = "windows")]
fn ensure_required_files(model_dir: &Path) -> Result<()> {
    for file in REQUIRED_FILES {
        let path = model_dir.join(file);
        if !path.is_file() {
            anyhow::bail!("missing required online model file: {}", path.display());
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn create_recognizer(model_dir: &Path) -> Result<OnlineRecognizer> {
    let mut config = OnlineRecognizerConfig::default();
    config.model_config.num_threads = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 4) as i32)
        .unwrap_or(2);
    config.model_config.provider = Some("cpu".into());
    config.model_config.tokens = Some(path_to_string(&model_dir.join("tokens.txt"))?);
    config.model_config.transducer.encoder = Some(path_to_string(
        &model_dir.join("encoder-epoch-99-avg-1.int8.onnx"),
    )?);
    config.model_config.transducer.decoder = Some(path_to_string(
        &model_dir.join("decoder-epoch-99-avg-1.onnx"),
    )?);
    config.model_config.transducer.joiner = Some(path_to_string(
        &model_dir.join("joiner-epoch-99-avg-1.int8.onnx"),
    )?);
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = 2.4;
    config.rule2_min_trailing_silence = 1.2;
    config.rule3_min_utterance_length = 20.0;
    config.decoding_method = Some("greedy_search".into());

    OnlineRecognizer::create(&config)
        .ok_or_else(|| anyhow::anyhow!("create sherpa-onnx online recognizer failed"))
}

#[cfg(target_os = "windows")]
fn read_audio(path: &Path) -> Result<(Vec<f32>, i32)> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "wav" => {
            let wave = Wave::read(&path_to_string(path)?)
                .ok_or_else(|| anyhow::anyhow!("read WAV failed: {}", path.display()))?;
            Ok((wave.samples().to_vec(), wave.sample_rate()))
        }
        "s16le" | "pcm" | "raw" => {
            let bytes =
                std::fs::read(path).with_context(|| format!("read raw PCM {}", path.display()))?;
            if bytes.len() % 2 != 0 {
                anyhow::bail!("raw PCM length is not aligned to i16 samples");
            }
            let samples = bytes
                .chunks_exact(2)
                .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0)
                .collect();
            Ok((samples, 16_000))
        }
        _ => anyhow::bail!("unsupported audio extension: {}", path.display()),
    }
}

#[cfg(target_os = "windows")]
fn capture_result(
    recognizer: &OnlineRecognizer,
    stream: &sherpa_onnx::OnlineStream,
    last_partial: &mut String,
    committed: &mut String,
) {
    let Some(result) = recognizer.get_result(stream) else {
        return;
    };
    let text = result.text.trim();
    if text.is_empty() || text == last_partial {
        return;
    }
    println!("partial: {text}");
    *last_partial = text.to_string();
    if result.is_final {
        append_segment(committed, text);
        println!("final-segment: {text}");
        last_partial.clear();
    }
}

#[cfg(target_os = "windows")]
fn append_segment(text: &mut String, segment: &str) {
    let segment = segment.trim();
    if segment.is_empty() {
        return;
    }
    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        text.push(' ');
    }
    text.push_str(segment);
}

#[cfg(target_os = "windows")]
fn path_to_string(path: &Path) -> Result<String> {
    Ok(path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))?
        .to_string())
}
