//! Polish / translate orchestration extracted from `coordinator.rs`
//! (behavior-preserving move).
//!
//! The streaming/one-shot polish entry points and the polish+translate combiner.
//! References parent items via `use super::*;`; `pub(super)` so the parent and
//! sibling submodules (e.g. `dictation`) reach them through `use polish_flow::*;`.

use super::*;

/// 润色文本；失败时返回原文 + 失败原因，调用方据此弹错误胶囊 + 写历史 error_code。
/// 之前固定返回 String，调用方拿不到失败信号 → 用户感知"为什么风格设置没生效"。issue #57。
/// 流式润色的三态结果。让上层（dictation pipeline）能区分「已经流出去了」、
/// 「降级到一次性」和「真失败了走 raw 兜底」三种 case。
pub enum StreamingPolishOutcome {
    /// 流式润色成功，`String` 是已经一边流一边交给 `on_delta` 的全部文本（用于写
    /// history、做词条命中统计）。调用方不应再 `inserter.insert(&text)`，因为字符
    /// 已经通过键盘事件落到光标处。
    Streamed(String),
    /// 当前配置不支持流式：用户没开 streaming_insert / Gemini provider / Codex
    /// provider / Raw 模式 / 翻译模式 / 不是 macOS。调用方应回到现有的
    /// `polish_or_passthrough` 一次性路径，跟历史行为完全一致。
    UnsupportedFallback,
    /// 流式过程中失败（HTTP / 解析 / 空流等）。`String` 是失败原因，调用方应当
    /// 走 raw 兜底（同 `polish_or_passthrough` 失败分支的语义）。
    Failed(String),
}

fn accumulate_llm_elapsed(total_ms: &mut Option<u64>, elapsed_ms: u64) {
    *total_ms = Some(total_ms.unwrap_or(0).saturating_add(elapsed_ms));
}

fn record_llm_elapsed(total_ms: &mut Option<u64>, started: std::time::Instant) {
    accumulate_llm_elapsed(total_ms, started.elapsed().as_millis() as u64);
}

/// 流式润色入口。在不支持流式的所有 case 都返回 `UnsupportedFallback`，让调用方
/// 透明降级。不修改任何持久化 / 焦点 / 光标状态。
///
/// `on_delta` 每收到一个 SSE chunk 就被调用一次（同步），调用方负责把 chunk 实际
/// 模拟键盘事件落到光标 —— 见 `coordinator/dictation.rs` 的流式分支。
/// `should_cancel` 用户取消时返回 true，立即 break SSE 读循环避免烧 quota。
pub async fn polish_or_passthrough_streaming<F, C>(
    raw: &RawTranscript,
    mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    cursor_context: Option<&str>,
    prior_turns: &[(String, String)],
    llm_call: &mut Option<crate::polish::LlmCallLabel>,
    llm_elapsed_ms: &mut Option<u64>,
    on_delta: F,
    should_cancel: C,
) -> StreamingPolishOutcome
where
    F: Fn(&str) + Send + Sync,
    C: Fn() -> bool + Send + Sync,
{
    if mode == PolishMode::Raw && !raw_mode_uses_llm(style_system_prompt) {
        log::info!("[coord] streaming polish skipped: mode=Raw, fall back to one-shot");
        return StreamingPolishOutcome::UnsupportedFallback;
    }
    let active_llm = CredentialsVault::get_active_llm();
    if active_llm == "gemini" {
        log::info!(
            "[coord] streaming polish skipped: active LLM provider=gemini (v1 not implemented), fall back to one-shot"
        );
        return StreamingPolishOutcome::UnsupportedFallback;
    }
    let provider = match build_active_llm_provider(llm_thinking_enabled) {
        Ok(p) => p,
        Err(e) => {
            log::error!("[coord] streaming polish: build provider failed: {e}");
            return StreamingPolishOutcome::Failed(e.to_string());
        }
    };
    if !provider.supports_streaming_polish() {
        log::info!(
            "[coord] streaming polish skipped: provider does not support streaming (likely codex OAuth), fall back to one-shot"
        );
        return StreamingPolishOutcome::UnsupportedFallback;
    }
    // 过了所有 early-out、即将发起真实调用——此刻才记录调用快照。
    *llm_call = Some(provider.call_label());
    log::info!(
        "[coord] streaming polish START: provider=openai-compatible mode={:?} raw_chars={} prior_turns={}",
        mode,
        raw.text.chars().count(),
        prior_turns.len()
    );
    let call_started = std::time::Instant::now();
    let result = provider
        .polish_streaming(
            &raw.text,
            mode,
            hotwords,
            style_system_prompt,
            working_languages,
            chinese_script_preference,
            output_language_preference,
            front_app,
            cursor_context,
            prior_turns,
            on_delta,
            should_cancel,
        )
        .await;
    record_llm_elapsed(llm_elapsed_ms, call_started);
    match result {
        Ok(text) => {
            log::info!(
                "[coord] streaming polish OK: final_chars={}",
                text.chars().count()
            );
            StreamingPolishOutcome::Streamed(text)
        }
        Err(e) => {
            let reason = e.to_string();
            log::error!("[coord] streaming polish FAILED: {reason}");
            StreamingPolishOutcome::Failed(reason)
        }
    }
}

pub(super) async fn polish_or_passthrough(
    raw: &RawTranscript,
    mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    cursor_context: Option<&str>,
    prior_turns: &[(String, String)],
    llm_call: &mut Option<crate::polish::LlmCallLabel>,
    llm_elapsed_ms: &mut Option<u64>,
    multimodal: bool,
) -> (String, Option<String>) {
    if mode == PolishMode::Raw && !raw_mode_uses_llm(style_system_prompt) {
        return (raw.text.clone(), None);
    }
    match polish_text(
        &raw.text,
        mode,
        hotwords,
        style_system_prompt,
        working_languages,
        chinese_script_preference,
        output_language_preference,
        llm_thinking_enabled,
        front_app,
        cursor_context,
        prior_turns,
        llm_call,
        llm_elapsed_ms,
        multimodal,
    )
    .await
    {
        Ok(s) => (s, None),
        Err(e) => {
            let reason = e.to_string();
            log::error!("[coord] polish failed, falling back to raw: {reason}");
            (raw.text.clone(), Some(reason))
        }
    }
}

pub(super) async fn polish_text(
    raw: &str,
    mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    cursor_context: Option<&str>,
    prior_turns: &[(String, String)],
    llm_call: &mut Option<crate::polish::LlmCallLabel>,
    llm_elapsed_ms: &mut Option<u64>,
    multimodal: bool,
) -> anyhow::Result<String> {
    // 多模态（Omni）模式：纯文本管线（选区润色 / 历史重润色）改用 omni 模型当
    // 文本 LLM，读取 omni 命名空间凭据，与传统 LLM 配置隔离。
    if multimodal {
        let provider = super::build_active_omni_provider(llm_thinking_enabled)?;
        let label = provider.call_label();
        *llm_call = Some(crate::polish::LlmCallLabel {
            provider: label.provider,
            model: label.model,
        });
        let mut system_prompt = style_system_prompt.to_string();
        if !hotwords.is_empty() {
            system_prompt.push_str(&format!(
                "\n\n# 词典/热词\n以下专有名词必须严格按给定写法准确识别：{}。",
                hotwords.join("、")
            ));
        }
        if !working_languages.is_empty() {
            system_prompt.push_str(&format!(
                "\n\n# 工作语言\n用户主要在以下语言间工作：{}。",
                working_languages.join("、")
            ));
        }
        let call_started = std::time::Instant::now();
        let result = provider.complete(&system_prompt, raw, None).await;
        record_llm_elapsed(llm_elapsed_ms, call_started);
        return Ok(result?);
    }

    // 谷歌 Gemini 分支：所有 LLM provider 共用 ark.* 凭据槽，唯独 Gemini 走原生
    // generateContent / 自带 thinkingConfig 控制；其余 provider 走 OpenAI
    // 兼容协议，并在该路径里按 provider/channel 下发对应的思考开关。
    let active_llm = CredentialsVault::get_active_llm();
    if active_llm == "gemini" {
        let (api_key, model, base_url) = read_gemini_credentials()?;
        // 凭据读取成功、即将发起调用——记录构建时快照（preflight 失败走上面的 ? 提前返回，不会记）。
        *llm_call = Some(crate::polish::LlmCallLabel {
            provider: active_llm.clone(),
            model: model.clone(),
        });
        let provider = GeminiProvider::new(
            GeminiConfig::new(api_key, model, base_url).with_thinking_enabled(llm_thinking_enabled),
        );
        let call_started = std::time::Instant::now();
        let result = provider
            .polish(
                raw,
                mode,
                hotwords,
                style_system_prompt,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
                cursor_context,
                prior_turns,
            )
            .await;
        record_llm_elapsed(llm_elapsed_ms, call_started);
        return Ok(result?);
    }

    let provider = build_active_llm_provider(llm_thinking_enabled)?;
    *llm_call = Some(provider.call_label());
    let call_started = std::time::Instant::now();
    let result = provider
        .polish(
            raw,
            mode,
            hotwords,
            style_system_prompt,
            working_languages,
            chinese_script_preference,
            output_language_preference,
            front_app,
            cursor_context,
            prior_turns,
        )
        .await;
    record_llm_elapsed(llm_elapsed_ms, call_started);
    Ok(result?)
}

/// 专用翻译（仅翻译、不润色、单轮）。现作为"润色+翻译"合成调用解析失败时的兜底——
/// 模型没按两段格式输出时，退回这里拿一段干净译文，而不是把畸形输出当译文插入。
pub(super) async fn translate_text(
    raw: &str,
    target_language: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    llm_call: &mut Option<crate::polish::LlmCallLabel>,
    llm_elapsed_ms: &mut Option<u64>,
) -> anyhow::Result<String> {
    // 见 polish_text 顶部注释——同样的 Gemini / OpenAI-compatible 路由逻辑。
    let active_llm = CredentialsVault::get_active_llm();
    if active_llm == "gemini" {
        let (api_key, model, base_url) = read_gemini_credentials()?;
        *llm_call = Some(crate::polish::LlmCallLabel {
            provider: active_llm.clone(),
            model: model.clone(),
        });
        let provider = GeminiProvider::new(
            GeminiConfig::new(api_key, model, base_url).with_thinking_enabled(llm_thinking_enabled),
        );
        let call_started = std::time::Instant::now();
        let result = provider
            .translate_to(
                raw,
                target_language,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
            )
            .await;
        record_llm_elapsed(llm_elapsed_ms, call_started);
        return Ok(result?);
    }

    let provider = build_active_llm_provider(llm_thinking_enabled)?;
    *llm_call = Some(provider.call_label());
    let call_started = std::time::Instant::now();
    let result = provider
        .translate_to(
            raw,
            target_language,
            working_languages,
            chinese_script_preference,
            output_language_preference,
            front_app,
        )
        .await;
    record_llm_elapsed(llm_elapsed_ms, call_started);
    Ok(result?)
}

/// "润色+翻译"单次调用的两段哨兵。模型按 `SRC\n源文\nTGT\n译文` 输出，解析器据此切分。
/// 这两个串必须与 build_polish_translate_system_prompt 写给模型的完全一致。
pub(super) const POLISH_TRANSLATE_SRC_MARKER: &str = "[[OPENLESS_POLISHED_SOURCE]]";
pub(super) const POLISH_TRANSLATE_TGT_MARKER: &str = "[[OPENLESS_TRANSLATION]]";

/// 合成"先润色源文、再翻译"的系统提示词：在原翻译 prompt 之上追加"额外输出润色后源文"
/// 与严格两段格式（覆盖原 prompt 末尾的"只输出译文"）。译文仍是要插入用户光标的主产物，
/// 故完整保留原翻译规则；润色后的源文只作对话上下文用，轻量清理即可。
pub(super) fn build_polish_translate_system_prompt(target_language: &str) -> String {
    let base = crate::polish::prompts::translate_system_prompt(target_language);
    format!(
        "{base}\n\n\
         # 额外输出：润色后的源文（仅用于对话上下文，不展示给用户）\n\
         在译文之前，先把上面的原始转写**按它本来的语言**润色一遍：去掉口癖（嗯 / 那个 / um）、\
         补必要标点、纠正明显的识别错误，但**不翻译、不改写风格、不增删意思**。\n\n\
         # 输出格式（覆盖上面\u{201C}只输出译文\u{201D}的说明，严格遵守）\n\
         严格按下面两段输出，两个标记必须原样出现、各占一行，标记之外不要有任何多余文字：\n\
         {src}\n\
         （这里放润色后的源文，保持原语言）\n\
         {tgt}\n\
         （这里放翻译成\u{300C}{lang}\u{300D}的译文）",
        base = base,
        src = POLISH_TRANSLATE_SRC_MARKER,
        tgt = POLISH_TRANSLATE_TGT_MARKER,
        lang = target_language,
    )
}

/// 解析"润色+翻译"单次调用输出 → Some((润色后源文, 译文))。
/// 找到译文标记且译文非空 → Some((源文, 译文))：源文标记缺失 / 源文段为空时源文为 None，
/// 译文取标记之后的干净正文。**没有译文标记、或译文段为空（模型截断 / 只吐了标记）→ None**，
/// 表示没拿到可信译文，交由调用方退回专用翻译——避免把空串当"成功译文"插进光标而丢字。
pub(super) fn split_polish_translate_output(raw: &str) -> Option<(Option<String>, String)> {
    let tgt_idx = raw.find(POLISH_TRANSLATE_TGT_MARKER)?;
    let translation = raw[tgt_idx + POLISH_TRANSLATE_TGT_MARKER.len()..]
        .trim()
        .to_string();
    if translation.is_empty() {
        return None;
    }
    let before_tgt = &raw[..tgt_idx];
    let source = before_tgt
        .find(POLISH_TRANSLATE_SRC_MARKER)
        .map(|i| {
            before_tgt[i + POLISH_TRANSLATE_SRC_MARKER.len()..]
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty());
    Some((source, translation))
}

/// 翻译路径——单次 LLM 调用同时润色源文 + 翻译。和 polish 一样失败时返回原文 + 失败原因，
/// 避免"不丢字"约定被违反（CLAUDE.md）。返回 (要插入的译文, 润色后源文供上下文用, 失败原因)。
#[allow(clippy::too_many_arguments)]
pub(super) async fn polish_and_translate_or_passthrough(
    raw: &RawTranscript,
    target_language: &str,
    mode: PolishMode,
    hotwords: &[String],
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    cursor_context: Option<&str>,
    prior_turns: &[(String, String)],
    llm_call: &mut Option<crate::polish::LlmCallLabel>,
    llm_elapsed_ms: &mut Option<u64>,
    multimodal: bool,
) -> (String, Option<String>, Option<String>) {
    let system_prompt = build_polish_translate_system_prompt(target_language);
    match polish_text(
        &raw.text,
        mode,
        hotwords,
        &system_prompt,
        working_languages,
        chinese_script_preference,
        output_language_preference,
        llm_thinking_enabled,
        front_app,
        cursor_context,
        prior_turns,
        llm_call,
        llm_elapsed_ms,
        multimodal,
    )
    .await
    {
        Ok(out) => match split_polish_translate_output(&out) {
            Some((source, translation)) => (translation, source, None),
            None => {
                // 模型没按两段格式输出：退回专用翻译拿一段干净译文，避免把畸形输出插进光标。
                // 此时无可信源文，这条翻译历史不参与后续普通润色上下文。
                log::warn!(
                    "[coord] polish+translate output missing markers; falling back to plain translate"
                );
                match translate_text(
                    &raw.text,
                    target_language,
                    working_languages,
                    chinese_script_preference,
                    output_language_preference,
                    llm_thinking_enabled,
                    front_app,
                    llm_call,
                    llm_elapsed_ms,
                )
                .await
                {
                    Ok(translation) => (translation, None, None),
                    Err(e) => {
                        let reason = e.to_string();
                        log::error!("[coord] fallback translate failed, using raw: {reason}");
                        (raw.text.clone(), None, Some(reason))
                    }
                }
            }
        },
        Err(e) => {
            let reason = e.to_string();
            log::error!("[coord] polish+translate failed, falling back to raw: {reason}");
            (raw.text.clone(), None, Some(reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PR #826 review：llm_call 快照只在真的构建 provider / 发起调用时填充。
    /// Raw 直通在读取任何凭据之前就 early-return，llm_call 必须保持 None——
    /// 调用方据此不落 llm_* / polish_ms。
    #[tokio::test]
    async fn raw_passthrough_leaves_llm_call_snapshot_empty() {
        let raw = RawTranscript {
            text: "原样输出".to_string(),
            duration_ms: 800,
        };
        let mut llm_call: Option<crate::polish::LlmCallLabel> = None;
        let mut llm_elapsed_ms = None;
        // 直通判定：style prompt 等于内置 raw 提示词 → raw_mode_uses_llm 为 false。
        let builtin_raw_prompt = crate::types::StyleSystemPrompts::default().raw;
        let (out, err) = polish_or_passthrough(
            &raw,
            PolishMode::Raw,
            &[],
            &builtin_raw_prompt,
            &[],
            ChineseScriptPreference::Auto,
            OutputLanguagePreference::Auto,
            false,
            None,
            None,
            &[],
            &mut llm_call,
            &mut llm_elapsed_ms,
            false,
        )
        .await;
        assert_eq!(out, "原样输出");
        assert_eq!(err, None);
        assert_eq!(llm_call, None, "Raw 直通不得产生 LLM 调用快照");
        assert_eq!(llm_elapsed_ms, None, "Raw 直通不得产生 LLM 调用耗时");
    }

    #[test]
    fn llm_elapsed_accumulates_only_provider_call_durations() {
        let mut elapsed_ms = None;
        accumulate_llm_elapsed(&mut elapsed_ms, 120);
        accumulate_llm_elapsed(&mut elapsed_ms, 80);
        assert_eq!(elapsed_ms, Some(200));
    }
}
