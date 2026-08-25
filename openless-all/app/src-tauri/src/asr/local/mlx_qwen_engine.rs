//! qwen3_asr_rs 的 MLX/Metal 包装。
//!
//! 上游库目前以音频文件作为输入。OpenLess 的录音器产生的是 16 kHz、单声道、
//! 16-bit PCM，因此这里只做一次临时 WAV 封装；模型本身保持驻留并跨会话复用。

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

use anyhow::{Context, Result};
use qwen3_asr_rs::inference::AsrInference;
use qwen3_asr_rs::tensor::Device;

// mlx-c 的默认错误处理器会调用 exit(-1)。这会把可恢复的 Metal/MLX
// 初始化错误伪装成“应用闪退”，而且 Rust 层完全没有机会记录上下文。
// 在 OpenLess 进程内改成只记录错误；调用方随后检查标志并返回 anyhow::Error。
type MlxErrorHandler = extern "C" fn(*const c_char, *mut c_void);

unsafe extern "C" {
    fn mlx_set_error_handler(
        handler: Option<MlxErrorHandler>,
        data: *mut c_void,
        dtor: Option<extern "C" fn(*mut c_void)>,
    );
}

static MLX_ERROR_HANDLER_ONCE: Once = Once::new();
static MLX_NATIVE_ERROR: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_mlx_error(message: *const c_char, _data: *mut c_void) {
    let message = if message.is_null() {
        "unknown MLX error".to_owned()
    } else {
        // SAFETY: mlx-c passes a valid, NUL-terminated error string for the
        // duration of this callback.
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    };
    MLX_NATIVE_ERROR.store(true, Ordering::Release);
    log::error!("[local-qwen3-mlx] native MLX error: {message}");
}

fn install_mlx_error_handler() {
    MLX_ERROR_HANDLER_ONCE.call_once(|| unsafe {
        mlx_set_error_handler(Some(handle_mlx_error), std::ptr::null_mut(), None);
    });
}

fn clear_mlx_native_error() {
    MLX_NATIVE_ERROR.store(false, Ordering::Release);
}

fn take_mlx_native_error() -> bool {
    MLX_NATIVE_ERROR.swap(false, Ordering::AcqRel)
}

pub struct MlxQwenAsrEngine {
    inference: Mutex<AsrInference>,
}

impl MlxQwenAsrEngine {
    pub fn load(model_dir: &Path) -> Result<Self> {
        ensure_tokenizer_json(model_dir)?;
        install_mlx_error_handler();
        clear_mlx_native_error();
        // qwen3_asr_rs 的 CLI 会在加载模型前做这一步；OpenLess 直接调用库 API，
        // 必须自行初始化全局 MLX stream，否则首次创建张量会 panic。
        qwen3_asr_rs::backend::mlx::stream::init_mlx(true);
        if take_mlx_native_error() {
            anyhow::bail!("MLX/Metal 初始化失败，请查看 native MLX error 日志");
        }
        log::info!(
            "[local-qwen3-mlx] loading model from {}",
            model_dir.display()
        );
        clear_mlx_native_error();
        let inference = AsrInference::load(model_dir, Device::gpu())
            .with_context(|| format!("加载 Qwen3-ASR MLX 模型失败: {}", model_dir.display()))?;
        if take_mlx_native_error() {
            anyhow::bail!("Qwen3-ASR MLX 加载触发 native MLX error");
        }
        Ok(Self {
            inference: Mutex::new(inference),
        })
    }

    pub fn transcribe_pcm(&self, samples: &[f32]) -> Result<String> {
        // 临时 WAV 用 guard 兜底清理：解码 panic、锁中毒提前 return 时也不会
        // 把文件泄漏到系统临时目录。
        let wav = TempWav::new(samples)?;
        let path_string = wav.path.to_string_lossy().into_owned();
        let output = self
            .inference
            .lock()
            .map_err(|_| anyhow::anyhow!("Qwen3-ASR MLX 引擎锁已中毒"))?
            .transcribe(&path_string, None)
            .context("Qwen3-ASR MLX batch 解码失败")?;
        Ok(output.text.trim().to_string())
    }
}

/// 临时 WAV 的 RAII 清理 guard：Drop 时删除文件。
struct TempWav {
    path: std::path::PathBuf,
}

impl TempWav {
    fn new(samples: &[f32]) -> Result<Self> {
        let path =
            std::env::temp_dir().join(format!("openless-qwen3-{}.wav", uuid::Uuid::new_v4()));
        let pcm: Vec<i16> = samples
            .iter()
            .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        std::fs::write(&path, crate::asr::wav::encode_wav_16k_mono(&pcm))
            .with_context(|| format!("写入临时 Qwen3-ASR 音频失败: {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Qwen 官方 ASR 权重通常只有 `vocab.json` + `merges.txt`，而 qwen3_asr_rs
/// 使用 HuggingFace 的统一 `tokenizer.json`。这里在首次加载时本地生成一次，
/// 避免要求用户安装 Python/Transformers；如果模型包已经带 tokenizer.json，则直接复用。
fn ensure_tokenizer_json(model_dir: &Path) -> Result<()> {
    let tokenizer_path = model_dir.join("tokenizer.json");
    if tokenizer_path.is_file() {
        return Ok(());
    }
    let vocab = model_dir.join("vocab.json");
    let merges = model_dir.join("merges.txt");
    let tokenizer_config = model_dir.join("tokenizer_config.json");
    if !vocab.is_file() || !merges.is_file() {
        anyhow::bail!(
            "Qwen3-ASR MLX 模型缺少 tokenizer.json、vocab.json 或 merges.txt: {}",
            model_dir.display()
        );
    }
    if !tokenizer_config.is_file() {
        anyhow::bail!(
            "Qwen3-ASR 模型缺少 tokenizer_config.json，无法恢复 added tokens: {}",
            model_dir.display()
        );
    }
    let vocab = vocab
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Qwen3-ASR vocab 路径不是有效 UTF-8"))?;
    let merges = merges
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Qwen3-ASR merges 路径不是有效 UTF-8"))?;
    let model = tokenizers::models::bpe::BPE::from_file(vocab, merges)
        .build()
        .map_err(|error| anyhow::anyhow!("生成 Qwen3-ASR BPE tokenizer 失败: {error}"))?;
    let mut tokenizer = tokenizers::Tokenizer::new(model);
    tokenizer.with_pre_tokenizer(Some(
        tokenizers::pre_tokenizers::byte_level::ByteLevel::default(),
    ));
    tokenizer.with_decoder(Some(tokenizers::decoders::byte_level::ByteLevel::default()));
    add_configured_tokens(&mut tokenizer, &tokenizer_config)?;
    validate_required_added_token(&tokenizer, "<asr_text>", 151704, false)?;
    let temporary = tokenizer_path.with_extension("json.partial");
    let tokenizer_json = tokenizer
        .to_string(false)
        .map_err(|error| anyhow::anyhow!("序列化 Qwen3-ASR tokenizer 失败: {error}"))?;
    std::fs::write(&temporary, tokenizer_json)
        .with_context(|| format!("写入 Qwen3-ASR tokenizer 失败: {}", temporary.display()))?;
    std::fs::rename(&temporary, &tokenizer_path).with_context(|| {
        format!(
            "提交 Qwen3-ASR tokenizer 失败: {} -> {}",
            temporary.display(),
            tokenizer_path.display()
        )
    })?;
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct TokenizerConfig {
    #[serde(default)]
    added_tokens_decoder: BTreeMap<String, AddedTokenConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct AddedTokenConfig {
    content: String,
    #[serde(default)]
    single_word: bool,
    #[serde(default)]
    lstrip: bool,
    #[serde(default)]
    rstrip: bool,
    #[serde(default = "default_normalized")]
    normalized: bool,
    #[serde(default)]
    special: bool,
}

fn default_normalized() -> bool {
    true
}

fn add_configured_tokens(
    tokenizer: &mut tokenizers::Tokenizer,
    tokenizer_config_path: &Path,
) -> Result<()> {
    let bytes = std::fs::read(tokenizer_config_path).with_context(|| {
        format!(
            "读取 Qwen3-ASR tokenizer_config.json 失败: {}",
            tokenizer_config_path.display()
        )
    })?;
    let config: TokenizerConfig = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "解析 Qwen3-ASR tokenizer_config.json 失败: {}",
            tokenizer_config_path.display()
        )
    })?;
    let mut entries = config
        .added_tokens_decoder
        .into_iter()
        .map(|(id, token)| {
            let id = id
                .parse::<u32>()
                .with_context(|| format!("Qwen3-ASR added token id 不是数字: {id}"))?;
            Ok((id, token))
        })
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by_key(|(id, _)| *id);
    if entries.is_empty() {
        anyhow::bail!("Qwen3-ASR tokenizer_config.json 缺少 added_tokens_decoder");
    }

    let base_vocab_size = tokenizer.get_vocab_size(false) as u32;
    for (index, (id, _)) in entries.iter().enumerate() {
        let expected_id = base_vocab_size + index as u32;
        if *id != expected_id {
            anyhow::bail!("Qwen3-ASR added token id 不连续: 期望 {expected_id}，实际 {id}");
        }
    }

    let added_tokens = entries
        .iter()
        .map(|(_, token)| {
            tokenizers::AddedToken::from(token.content.clone(), token.special)
                .single_word(token.single_word)
                .lstrip(token.lstrip)
                .rstrip(token.rstrip)
                .normalized(token.normalized)
        })
        .collect::<Vec<_>>();
    tokenizer.add_tokens(&added_tokens);

    for ((id, config), added) in entries.iter().zip(added_tokens.iter()) {
        if tokenizer.token_to_id(&config.content) != Some(*id) {
            anyhow::bail!(
                "Qwen3-ASR added token id 对齐失败: {} 应为 {id}",
                added.content
            );
        }
    }
    Ok(())
}

fn validate_required_added_token(
    tokenizer: &tokenizers::Tokenizer,
    content: &str,
    expected_id: u32,
    expected_special: bool,
) -> Result<()> {
    let decoder = tokenizer.get_added_tokens_decoder();
    let token = decoder.get(&expected_id).ok_or_else(|| {
        anyhow::anyhow!("Qwen3-ASR tokenizer 缺少 added token: {content} ({expected_id})")
    })?;
    if token.content != content || token.special != expected_special {
        anyhow::bail!(
            "Qwen3-ASR added token 配置不匹配: id={expected_id}, content={}, special={}",
            token.content,
            token.special
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{add_configured_tokens, validate_required_added_token};

    #[test]
    fn preserves_non_special_added_token_ids() {
        let dir = std::env::temp_dir().join(format!(
            "openless-qwen-tokenizer-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let vocab = dir.join("vocab.json");
        let merges = dir.join("merges.txt");
        let config = dir.join("tokenizer_config.json");
        std::fs::write(&vocab, r#"{"a": 0}"#).unwrap();
        std::fs::write(&merges, "#version: 0.2\n").unwrap();
        std::fs::write(
            &config,
            r#"{"added_tokens_decoder":{"1":{"content":"<asr_text>","special":false}}}"#,
        )
        .unwrap();

        let vocab_path = vocab.to_str().unwrap();
        let merges_path = merges.to_str().unwrap();
        let model = tokenizers::models::bpe::BPE::from_file(vocab_path, merges_path)
            .build()
            .unwrap();
        let mut tokenizer = tokenizers::Tokenizer::new(model);
        add_configured_tokens(&mut tokenizer, &config).unwrap();
        validate_required_added_token(&tokenizer, "<asr_text>", 1, false).unwrap();

        assert_eq!(tokenizer.token_to_id("<asr_text>"), Some(1));
        assert_eq!(
            tokenizer
                .get_added_tokens_decoder()
                .get(&1)
                .map(|token| token.special),
            Some(false)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
