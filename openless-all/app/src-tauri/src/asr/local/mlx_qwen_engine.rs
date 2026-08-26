//! qwen3_asr_rs 的 MLX/Metal 包装。
//!
//! 上游库目前以音频文件作为输入。OpenLess 的录音器产生的是 16 kHz、单声道、
//! 16-bit PCM，因此这里只做一次临时 WAV 封装；模型本身保持驻留并跨会话复用。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use super::mlx_worker::MlxWorkerClient;
use anyhow::{Context, Result};

pub struct MlxQwenAsrEngine {
    worker: MlxWorkerClient,
}

impl MlxQwenAsrEngine {
    pub fn load(model_dir: &Path) -> Result<Self> {
        Ok(Self {
            worker: MlxWorkerClient::load(model_dir)?,
        })
    }

    pub fn transcribe_pcm(&self, samples: &[f32]) -> Result<String> {
        self.worker.transcribe_pcm(samples)
    }

    pub fn next_operation_id(&self) -> u64 {
        self.worker.next_operation_id()
    }

    pub fn transcribe_pcm_for_operation(
        &self,
        operation_id: u64,
        samples: &[f32],
        cancelled: &AtomicBool,
    ) -> Result<String> {
        self.worker
            .transcribe_pcm_for_operation(operation_id, samples, cancelled)
    }

    pub fn cancel_operation(&self, operation_id: u64) {
        self.worker.cancel_operation(operation_id);
    }

    pub fn abort(&self) {
        self.worker.abort();
    }

    pub fn is_healthy(&self) -> bool {
        self.worker.is_healthy()
    }
}

/// Qwen 官方 ASR 权重通常只有 `vocab.json` + `merges.txt`，而 qwen3_asr_rs
/// 使用 HuggingFace 的统一 `tokenizer.json`。这里在首次加载时本地生成一次，
/// 避免要求用户安装 Python/Transformers；如果模型包已经带 tokenizer.json，则直接复用。
pub(super) fn ensure_tokenizer_json(model_dir: &Path) -> Result<()> {
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
