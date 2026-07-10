//! Batch Whisper ASR client — collects PCM in a buffer, then POSTs a WAV file
//! to any OpenAI-compatible `/audio/transcriptions` endpoint on session end.

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

const PCM_SAMPLE_RATE_HZ: u64 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;

/// Whisper の `prompt` パラメータの安全側上限（文字数）。
///
/// OpenAI / Groq の Audio Transcriptions API は `prompt` を 244 トークンまで
/// 受け付ける。トークナイザは BPE で言語によって 1 token あたりの文字数が
/// 異なる：英語は ~4 chars/token、日本語・中国語は最悪 ~1 char/token。
/// CJK ユーザーが安全に収まるよう、文字数で 240 を上限にする。
pub const PROMPT_CHAR_BUDGET: usize = 240;

/// 区切り文字（ASCII）。Whisper のトークナイザはどの言語でも安定して扱える。
const PROMPT_SEPARATOR: &str = ", ";

/// `/audio/transcriptions` 请求体编码方式。
///
/// OpenAI 官方及多数兼容厂商用 `multipart/form-data`（file + model）。
/// OpenRouter 虽路径相同、也走 Bearer，但请求体是 `application/json`：
/// `{model, input_audio:{data:<base64 wav>, format:"wav"}}`（issue #582）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsrRequestFormat {
    /// `multipart/form-data`（既有行为，默认）。
    Multipart,
    /// OpenRouter `application/json` + base64 音频。
    OpenRouterJson,
}

pub struct WhisperBatchASR {
    api_key: String,
    base_url: String,
    model: String,
    /// 任意のプロンプト（語彙ヒント等）。空文字や空白のみは送信しない。
    /// `None` ＝ プロンプト無し（既存挙動）。
    prompt: Option<String>,
    /// OpenAI 互換でもファイル長に上限がある provider 用。None は従来通り一括送信。
    max_chunk_duration_ms: Option<u64>,
    /// `response_format=verbose_json` を要求してセグメント単位のメタデータ
    /// （no_speech_prob / avg_logprob / compression_ratio）で幻聴を捨てるか。
    /// OpenAI / Groq の Whisper は full に対応。SenseVoice / TeleSpeech 等
    /// （SiliconFlow）は response_format 自体が無いので false にして従来の
    /// `json` のまま送る（壊さない）。
    verbose_json: bool,
    /// 请求体编码方式。默认 `Multipart`，OpenRouter 走 `OpenRouterJson`。
    request_format: AsrRequestFormat,
    buffer: Mutex<Vec<u8>>,
}

impl WhisperBatchASR {
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        prompt: Option<String>,
        max_chunk_duration_ms: Option<u64>,
        verbose_json: bool,
    ) -> Self {
        Self {
            api_key,
            base_url,
            model,
            prompt,
            max_chunk_duration_ms,
            verbose_json,
            request_format: AsrRequestFormat::Multipart,
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// 设置请求体编码方式（默认 `Multipart`）。OpenRouter 需 `OpenRouterJson`。
    /// 用 builder 而非给 `new()` 加参数，避免改动既有 4 处构造点的签名。
    pub fn with_request_format(mut self, request_format: AsrRequestFormat) -> Self {
        self.request_format = request_format;
        self
    }

    /// Stop collecting audio, encode the buffer as WAV, and POST to the
    /// Whisper transcriptions endpoint.
    ///
    /// 失败时**保留** PCM buffer，让上层有机会重试或在历史中至少留一个失败记录；
    /// 当前缓冲音频时长（毫秒）。Coordinator 在 transcribe() 调用前读取，
    /// 用于计算 Whisper / OpenRouter 的动态超时。不消费缓冲。
    pub fn buffer_duration_ms(&self) -> u64 {
        pcm_duration_ms(&self.buffer.lock())
    }

    /// 之前的实现一进函数就 `mem::take` 把 buffer 清空，凭证错或网络中断都会
    /// 让用户的录音直接消失。
    pub async fn transcribe(&self) -> Result<RawTranscript> {
        // clone 而不是 take：~30s 16 kHz 16-bit 音频 ≈ 960 KB，会话末调用一次，可接受。
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let result = self.transcribe_inner(&pcm).await;
        // 仅在成功路径上才清 buffer。失败时 PCM 还在，coordinator 拿到 Err 但
        // 用户重新触发 stop 时仍能再发一次，或日后增加重试入口时复用。
        if result.is_ok() {
            self.buffer.lock().clear();
        }
        result
    }

    async fn transcribe_inner(&self, pcm: &[u8]) -> Result<RawTranscript> {
        if self.api_key.is_empty() {
            anyhow::bail!("Whisper API key missing");
        }

        let duration_ms = pcm_duration_ms(pcm);
        let chunks = split_pcm_by_duration(pcm, self.max_chunk_duration_ms);
        let mut texts = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            texts.push(self.transcribe_chunk(chunk).await?);
        }

        Ok(RawTranscript {
            text: join_transcript_chunks(&texts),
            duration_ms,
        })
    }

    async fn transcribe_chunk(&self, pcm: &[u8]) -> Result<String> {
        let samples: Vec<i16> = pcm
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let wav = encode_wav_16k_mono(&samples);
        let url = transcription_url(&self.base_url)?;
        let client = reqwest::Client::new();

        let request = match self.request_format {
            AsrRequestFormat::Multipart => {
                let wav_part = reqwest::multipart::Part::bytes(wav)
                    .file_name("audio.wav")
                    .mime_str("audio/wav")
                    .context("set MIME type")?;
                let mut form = reqwest::multipart::Form::new()
                    .part("file", wav_part)
                    .text("model", self.model.clone());

                // verbose_json 対応プロバイダ（OpenAI / Groq）のときだけ、セグメント
                // メタデータ付きの応答を要求し、temperature も 0 に固定する。非対応
                // プロバイダ（SiliconFlow の SenseVoice / TeleSpeech 等）には送らず
                // 従来どおりの応答にして、未知パラメータでの 4xx を避ける。
                if self.verbose_json {
                    form = form
                        .text("response_format", "verbose_json")
                        .text("temperature", "0");
                }

                // `prompt` は空文字を送らない：OpenAI 互換実装によっては空文字でエラーに
                // なるリスクがある（Groq は許容するが防御的にスキップ）。`trim()` で
                // 空白のみのケースも除外。
                if let Some(prompt) = self.prompt.as_ref() {
                    let trimmed = prompt.trim();
                    if !trimmed.is_empty() {
                        form = form.text("prompt", trimmed.to_string());
                    }
                }

                client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .multipart(form)
            }
            AsrRequestFormat::OpenRouterJson => {
                // OpenRouter /audio/transcriptions：application/json，音频走标准
                // base64（带 padding）。不带 multipart 专属的 prompt/response_format
                // 字段，避免未知字段导致 4xx；verbose_json 对该协议保持关闭。
                let body = serde_json::json!({
                    "model": self.model,
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(&wav),
                        "format": "wav",
                    },
                });
                client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&body)
            }
        };

        let resp = request
            .send()
            .await
            .context("Whisper HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Whisper API error {}: {}", status, body);
        }

        let json: serde_json::Value = resp.json().await.context("parse Whisper response")?;
        if self.verbose_json {
            // verbose_json：セグメントのメタデータで幻聴を除いた本文を組む。
            // segments が無い応答では内部で従来どおり text にフォールバック。
            Ok(extract_confident_text(&json))
        } else {
            Ok(json["text"].as_str().unwrap_or("").trim().to_string())
        }
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

impl crate::recorder::AudioConsumer for WhisperBatchASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// verbose_json 应答里去掉「幻听」段落后拼出正文。
///
/// Whisper 在静音 / 弱音 / 噪声段会生成「听起来合理但用户没说」的文本（已知
/// hallucination 缺陷）：录音前后的沉默或麦克风底噪会变成无关词。verbose_json
/// 的每个 segment 带 `no_speech_prob` / `avg_logprob` / `compression_ratio`，
/// 用它们丢掉明显不是真实语音的段落。
///
/// 判定（命中任一即丢弃）：
/// - `no_speech_prob > 0.6` 且 `avg_logprob < -0.5`：高静音概率且低置信，沉默被作话。
/// - `compression_ratio > 2.4`：同一短语反复幻听（Whisper 标准阈值）。
/// - `avg_logprob < -1.0`：置信极低，噪声被词化。
///
/// 误删真实语音最糟，所以阈值保守。没有 `segments` 字段（例如 provider 忽略了
/// verbose_json）时退回直接用 `text`，与旧行为一致。元数据字段缺失时按
/// 「不丢弃」处理（unwrap_or 默认值），所以对不返回这些指标的 provider 是无害空转。
fn extract_confident_text(json: &serde_json::Value) -> String {
    let Some(segments) = json.get("segments").and_then(|s| s.as_array()) else {
        return json["text"].as_str().unwrap_or("").trim().to_string();
    };

    let mut kept = String::new();
    for seg in segments {
        let text = seg.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if text.trim().is_empty() {
            continue;
        }
        let no_speech = seg
            .get("no_speech_prob")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let avg_logprob = seg
            .get("avg_logprob")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let compression = seg
            .get("compression_ratio")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        let is_hallucination =
            (no_speech > 0.6 && avg_logprob < -0.5) || compression > 2.4 || avg_logprob < -1.0;
        if is_hallucination {
            log::warn!(
                "[whisper] 丢弃疑似幻听段落: no_speech={:.2} avg_logprob={:.2} compression={:.2} text={:?}",
                no_speech,
                avg_logprob,
                compression,
                text.trim()
            );
            continue;
        }
        kept.push_str(text);
    }

    let kept = kept.trim().to_string();
    if kept.is_empty() {
        // 全部段落被判为幻听（≈整段几乎是静音）。回退到原始 text 会把幻听又捡
        // 回来，所以返回空串；上层把空转写当「什么都没说」无害处理。
        return String::new();
    }
    kept
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    super::pcm::pcm_duration_ms(pcm)
}

fn split_pcm_by_duration(pcm: &[u8], max_chunk_duration_ms: Option<u64>) -> Vec<&[u8]> {
    let Some(max_chunk_duration_ms) = max_chunk_duration_ms else {
        return vec![pcm];
    };
    if max_chunk_duration_ms == 0 {
        return vec![pcm];
    }

    let samples_per_chunk = PCM_SAMPLE_RATE_HZ * max_chunk_duration_ms / 1000;
    let bytes_per_chunk = samples_per_chunk as usize * PCM_BYTES_PER_SAMPLE;
    if bytes_per_chunk == 0 || pcm.len() <= bytes_per_chunk {
        return vec![pcm];
    }

    pcm.chunks(bytes_per_chunk).collect()
}

fn transcription_url(base_url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(base_url.trim()).context("parse Whisper base URL")?;
    let mut url = parsed.clone();
    let path = parsed.path().trim_end_matches('/');
    let next_path = if path.ends_with("/audio/transcriptions") {
        path.to_string()
    } else if path.ends_with("/audio") {
        format!("{path}/transcriptions")
    } else if let Some(prefix) = path.strip_suffix("/chat/completions") {
        format!("{prefix}/audio/transcriptions")
    } else {
        format!("{path}/audio/transcriptions")
    };
    url.set_path(&next_path);
    Ok(url.to_string())
}

fn join_transcript_chunks(chunks: &[String]) -> String {
    let mut joined = String::new();
    for chunk in chunks.iter().map(|chunk| chunk.trim()) {
        if chunk.is_empty() {
            continue;
        }
        if needs_chunk_separator(&joined, chunk) {
            joined.push(' ');
        }
        joined.push_str(chunk);
    }
    joined
}

fn needs_chunk_separator(current: &str, next: &str) -> bool {
    let Some(prev) = current.chars().last() else {
        return false;
    };
    let Some(first) = next.chars().next() else {
        return false;
    };

    if is_closing_punctuation(first) || is_opening_punctuation(prev) {
        return false;
    }
    if is_cjk(prev) && (is_cjk(first) || is_opening_punctuation(first)) {
        return false;
    }
    if is_cjk(first) && is_closing_punctuation(prev) {
        return false;
    }
    if is_cjk_punctuation(prev) && is_cjk(first) {
        return false;
    }
    true
}

fn is_opening_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '(' | '[' | '{' | '"' | '\'' | '（' | '「' | '『' | '《' | '“' | '‘'
    )
}

fn is_closing_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ',' | '.'
            | '!'
            | '?'
            | ':'
            | ';'
            | ')'
            | ']'
            | '}'
            | '"'
            | '\''
            | '，'
            | '。'
            | '、'
            | '！'
            | '？'
            | '：'
            | '；'
            | '）'
            | '」'
            | '』'
            | '》'
            | '”'
            | '’'
            | '…'
    )
}

fn is_cjk_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '，' | '。'
            | '、'
            | '！'
            | '？'
            | '：'
            | '；'
            | '（'
            | '）'
            | '「'
            | '」'
            | '『'
            | '』'
            | '《'
            | '》'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '…'
            | '—'
    )
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
    )
}

/// 用户辞書の有効フレーズから Whisper の `prompt` パラメータを組み立てる。
///
/// Whisper は `prompt` で語彙ヒント / スタイル文脈を渡せる：固有名詞・専門
/// 用語の表記揺れを抑え、ASR 段階で正しい綴り（漢字選択を含む）に偏らせる。
/// 既存の dictionary 機能はこれまで Volcengine ASR と Polish LLM のみに渡って
/// いて、Whisper 互換プロバイダ（whisper / siliconflow / zhipu / groq）には
/// 流れていなかった。本関数で同じエントリを Whisper にも届ける。
///
/// # 仕様
///
/// - 空白のみのフレーズは除外
/// - 区切りは `, `
/// - 末尾に `.` を付与して「文の終わり」を Whisper に明示（モデルがプロンプト
///   を続きと誤解して書き起こし冒頭に混入するのを抑える）
/// - 文字数が `PROMPT_CHAR_BUDGET` を超えるエントリは**スキップ**して次に
///   進む（途中で打ち切らない）。これにより「先頭に長文 1 件があると残りが
///   全部捨てられる」現象を回避でき、登録順を保ちつつ収まるエントリを最大化
///   できる。
/// - 入力が空、または有効フレーズが 0 件の場合は `None` を返す。Optional に
///   することで「プロンプト無し」と「空文字プロンプト」を呼び出し側で区別
///   する必要をなくす。
pub fn build_prompt_from_phrases(phrases: &[String]) -> Option<String> {
    let mut included: Vec<&str> = Vec::new();
    let mut total_chars: usize = 0;

    for phrase in phrases {
        let trimmed = phrase.trim();
        if trimmed.is_empty() {
            continue;
        }
        let phrase_chars = trimmed.chars().count();
        let added = if included.is_empty() {
            phrase_chars
        } else {
            PROMPT_SEPARATOR.chars().count() + phrase_chars
        };
        // 末尾の "." 1 文字も予約。
        if total_chars + added + 1 > PROMPT_CHAR_BUDGET {
            continue;
        }
        included.push(trimmed);
        total_chars += added;
    }

    if included.is_empty() {
        return None;
    }
    let mut s = included.join(PROMPT_SEPARATOR);
    s.push('.');
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn build_prompt_returns_none_for_empty_input() {
        assert_eq!(build_prompt_from_phrases(&[]), None);
    }

    #[test]
    fn build_prompt_returns_none_when_all_phrases_blank() {
        let phrases = vec!["".to_string(), "   ".to_string(), "\t\n".to_string()];
        assert_eq!(build_prompt_from_phrases(&phrases), None);
    }

    #[test]
    fn build_prompt_single_phrase() {
        let phrases = vec!["梁山泊".to_string()];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊.".to_string())
        );
    }

    #[test]
    fn build_prompt_joins_with_comma_and_appends_period() {
        let phrases = vec![
            "梁山泊".to_string(),
            "片沼ほとり".to_string(),
            "TRC".to_string(),
        ];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊, 片沼ほとり, TRC.".to_string())
        );
    }

    #[test]
    fn build_prompt_trims_each_phrase() {
        let phrases = vec!["  梁山泊  ".to_string(), "\tTRC\n".to_string()];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊, TRC.".to_string())
        );
    }

    #[test]
    fn build_prompt_skips_blank_entries_in_middle() {
        let phrases = vec![
            "alpha".to_string(),
            "".to_string(),
            "   ".to_string(),
            "beta".to_string(),
        ];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("alpha, beta.".to_string())
        );
    }

    #[test]
    fn build_prompt_truncates_overflow_but_keeps_short_entries_after_long_one() {
        // 先頭に 250 文字の長文 → 単独で予算超過 → スキップ。続く短いエントリは
        // 採用される。「途中で break しない」契約の検証。
        let long = "あ".repeat(250);
        let phrases = vec![long.clone(), "梁山泊".to_string(), "TRC".to_string()];
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(!prompt.contains(&long), "long phrase must be dropped");
        assert!(prompt.contains("梁山泊"));
        assert!(prompt.contains("TRC"));
        assert!(prompt.ends_with('.'));
    }

    #[test]
    fn build_prompt_respects_char_budget() {
        // 6 文字 × 50 件 = 300 文字（区切り込みでさらに増える）→ 予算超過分は捨てる。
        let phrases: Vec<String> = (0..50).map(|i| format!("word{:02}", i)).collect();
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(
            prompt.chars().count() <= PROMPT_CHAR_BUDGET,
            "prompt length {} exceeds budget {}",
            prompt.chars().count(),
            PROMPT_CHAR_BUDGET
        );
        assert!(prompt.ends_with('.'));
    }

    #[test]
    fn build_prompt_includes_first_entries_when_truncating_in_order() {
        // 順序保証：登録順の早いものから入る。後続が落ちる。
        let phrases: Vec<String> = (0..100).map(|i| format!("entry{:03}", i)).collect();
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(prompt.contains("entry000"));
        assert!(prompt.contains("entry001"));
        // 100 件 × 8 文字以上は確実に予算超過 → 末尾は入らない
        assert!(!prompt.contains("entry099"));
    }

    #[test]
    fn split_pcm_by_duration_keeps_default_as_single_chunk() {
        let pcm = vec![0u8; 96_000];
        assert_eq!(split_pcm_by_duration(&pcm, None), vec![pcm.as_slice()]);
    }

    #[test]
    fn split_pcm_by_duration_uses_sample_boundaries() {
        let pcm = vec![0u8; 32_000 * 65];
        let chunks = split_pcm_by_duration(&pcm, Some(30_000));

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 32_000 * 30);
        assert_eq!(chunks[1].len(), 32_000 * 30);
        assert_eq!(chunks[2].len(), 32_000 * 5);
    }

    #[test]
    fn split_pcm_by_duration_zero_limit_falls_back_to_single_chunk() {
        let pcm = vec![0u8; 96_000];
        assert_eq!(split_pcm_by_duration(&pcm, Some(0)), vec![pcm.as_slice()]);
    }

    #[test]
    fn transcription_url_accepts_base_audio_or_full_endpoint() {
        assert_eq!(
            transcription_url("https://open.bigmodel.cn/api/paas/v4").unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions"
        );
        assert_eq!(
            transcription_url("https://open.bigmodel.cn/api/paas/v4/audio").unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions"
        );
        assert_eq!(
            transcription_url("https://open.bigmodel.cn/api/paas/v4/audio/transcriptions").unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions"
        );
        assert_eq!(
            transcription_url(
                "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions?api-version=2026-01-01"
            )
            .unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions?api-version=2026-01-01"
        );
    }

    #[test]
    fn join_transcript_chunks_skips_empty_chunks() {
        let chunks = vec![" hello ".to_string(), "".to_string(), "world".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "hello world");
    }

    #[test]
    fn join_transcript_chunks_keeps_cjk_together() {
        let chunks = vec!["你好".to_string(), "世界".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "你好世界");
    }

    #[test]
    fn join_transcript_chunks_separates_mixed_script_boundaries() {
        let chunks = vec!["中文".to_string(), "English".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "中文 English");

        let chunks = vec!["OpenLess".to_string(), "中文".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "OpenLess 中文");
    }

    #[test]
    fn join_transcript_chunks_handles_punctuation_boundaries() {
        let chunks = vec!["hello,".to_string(), "world".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "hello, world");

        let chunks = vec!["hello".to_string(), ",".to_string(), "world".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "hello, world");

        let chunks = vec!["foo.".to_string(), "bar".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "foo. bar");

        let chunks = vec!["(".to_string(), "hello".to_string(), ")".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "(hello)");
    }

    #[test]
    fn join_transcript_chunks_handles_cjk_punctuation_boundaries() {
        let chunks = vec!["你好".to_string(), "，世界".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "你好，世界");

        let chunks = vec!["中文".to_string(), "。".to_string(), "下一句".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "中文。下一句");

        let chunks = vec!["他说".to_string(), "：".to_string(), "你好".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "他说：你好");

        let chunks = vec!["中文。".to_string(), "OpenAI".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "中文。 OpenAI");

        let chunks = vec!["「".to_string(), "中文".to_string(), "」".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "「中文」");
    }

    #[test]
    fn extract_confident_text_drops_hallucinated_segment() {
        let json = serde_json::json!({
            "text": "本当の発話 幻聴",
            "segments": [
                {"text": "本当の発話", "no_speech_prob": 0.01, "avg_logprob": -0.2, "compression_ratio": 1.2},
                {"text": "幻聴", "no_speech_prob": 0.9, "avg_logprob": -0.8, "compression_ratio": 1.1},
            ]
        });
        assert_eq!(extract_confident_text(&json), "本当の発話");
    }

    #[test]
    fn extract_confident_text_keeps_all_confident_segments() {
        let json = serde_json::json!({
            "text": "ignored",
            "segments": [
                {"text": "前半", "no_speech_prob": 0.0, "avg_logprob": -0.1, "compression_ratio": 1.0},
                {"text": "後半", "no_speech_prob": 0.0, "avg_logprob": -0.2, "compression_ratio": 1.0},
            ]
        });
        assert_eq!(extract_confident_text(&json), "前半後半");
    }

    #[test]
    fn extract_confident_text_falls_back_to_text_without_segments() {
        let json = serde_json::json!({ "text": "  素の文字起こし  " });
        assert_eq!(extract_confident_text(&json), "素の文字起こし");
    }

    #[test]
    fn extract_confident_text_missing_metrics_keeps_segment() {
        // provider が指標を返さない場合は「不丢弃」＝そのまま残す（無害空転）。
        let json = serde_json::json!({
            "text": "x",
            "segments": [ {"text": "保留される"} ]
        });
        assert_eq!(extract_confident_text(&json), "保留される");
    }

    #[tokio::test]
    async fn transcribe_posts_single_request_without_chunk_limit() {
        let (base_url, server) = start_whisper_test_server(vec!["one"]);
        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "model".to_string(),
            None,
            None,
            false,
        );
        let pcm = vec![0u8; 32_000 * 65];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "one");
        assert_eq!(transcript.duration_ms, 65_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn transcribe_splits_requests_when_chunk_limit_is_set() {
        let (base_url, server) = start_whisper_test_server(vec!["你好", "world", "尾"]);
        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "model".to_string(),
            None,
            Some(30_000),
            false,
        );
        let pcm = vec![0u8; 32_000 * 65];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "你好 world 尾");
        assert_eq!(transcript.duration_ms, 65_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn openrouter_format_posts_json_with_base64_audio() {
        // issue #582：OpenRouterJson 走 application/json + input_audio.data(base64)，
        // 而非 multipart；响应仍按 {text} 解析。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            let lower = request_text.to_ascii_lowercase();
            assert!(request_text.starts_with("POST /audio/transcriptions HTTP/1.1"));
            assert!(lower.contains("content-type: application/json"));
            assert!(lower.contains("authorization: bearer key"));
            // body 是 JSON：含 input_audio.data + format:"wav"，且不是 multipart。
            assert!(request_text.contains("input_audio"));
            assert!(request_text.contains(r#""format":"wav""#));
            assert!(!lower.contains("multipart/form-data"));
            write_json_response(&mut stream, r#"{"text":"openrouter ok"}"#);
        });
        let base_url = format!("http://{}", addr);

        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "openai/whisper-large-v3-turbo".to_string(),
            None,
            None,
            false,
        )
        .with_request_format(AsrRequestFormat::OpenRouterJson);
        let pcm = vec![0u8; 32_000 * 2];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "openrouter ok");
        server.join().unwrap();
    }

    fn start_whisper_test_server(texts: Vec<&'static str>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for text in texts {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "timed out waiting for ASR test request"
                            );
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("accept ASR test request failed: {err}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = read_http_request(&mut stream);
                let request_text = String::from_utf8_lossy(&request);
                let request_text_lower = request_text.to_ascii_lowercase();
                assert!(request_text.starts_with("POST /audio/transcriptions HTTP/1.1"));
                assert!(request_text_lower.contains("authorization: bearer key"));
                assert!(request_text.contains("model"));
                write_json_response(&mut stream, &format!(r#"{{"text":"{}"}}"#, text));
            }
        });
        (format!("http://{}", addr), server)
    }

    fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut buf = [0u8; 8192];
        let mut request = Vec::new();
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            let Some(header_end) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let header_text = String::from_utf8_lossy(&request[..header_end + 4]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .or_else(|| line.strip_prefix("Content-Length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        request
    }

    fn write_json_response(stream: &mut TcpStream, body: &str) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    }
}
