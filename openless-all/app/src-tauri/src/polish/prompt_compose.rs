//! Polish / translate / QA system-prompt composition extracted from `polish.rs`
//! (behavior-preserving move).
//!
//! Builds the context premise, hotword block, and the assembled system/user
//! prompts. References the parent `prompts` module and `PolishSystemPromptAssembly`
//! via `use super::*;`. `pub(crate)` fns are re-exported from `polish`.

use super::*;

/// 把 working_languages + front_app 拼成 system prompt 头部前提：
///     # 上下文
///     用户的工作语言：…
///     当前前台应用：…（请按这个 app 的常见沟通风格调整语气）
///
/// 两个字段都空时返回 None，调用方就不拼前缀。详见 issue #4 / #116。
pub(super) fn context_premise(
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    front_app: Option<&str>,
) -> Option<String> {
    let langs: Vec<&str> = working_languages
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    // 安全：window title 是攻击者可控字段，嵌入前必须清理。
    // 去除换行符（防止注入多行指令）和 Markdown/XML 分隔符（防止结构性提示注入）；
    // 截断到 100 个字符（远超任何真实 app 名称的合理长度）。
    let app = front_app
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let sanitized: String = s
                .chars()
                .filter(|c| *c != '\n' && *c != '\r' && *c != '#' && *c != '<' && *c != '>')
                .take(100)
                .collect();
            sanitized
        })
        .filter(|s| !s.is_empty());

    let script_line = match chinese_script_preference {
        ChineseScriptPreference::Simplified => Some(
            "中文输出偏好：简体中文。若最终输出包含中文，请统一使用简体字形（不要混用繁体）。"
                .to_string(),
        ),
        ChineseScriptPreference::Traditional => Some(
            "中文输出偏好：繁体中文。若最终输出包含中文，请统一使用繁体字形（不要混用简体）。"
                .to_string(),
        ),
        ChineseScriptPreference::Auto => None,
    };

    let output_language_line = match output_language_preference {
        OutputLanguagePreference::ZhCn => {
            Some("最终输出语言偏好：简体中文。若回答可用中文表达，请优先使用简体中文。".to_string())
        }
        OutputLanguagePreference::ZhTw => {
            Some("最終輸出語言偏好：繁體中文。若回答可用中文表達，請優先使用繁體中文。".to_string())
        }
        OutputLanguagePreference::En => Some(
            "Output language preference: English. Prefer English when producing the final answer."
                .to_string(),
        ),
        OutputLanguagePreference::Ja => Some(
            "出力言語の優先設定：日本語。最終回答は可能な限り日本語で出力してください。"
                .to_string(),
        ),
        OutputLanguagePreference::Ko => {
            Some("출력 언어 선호: 한국어. 최종 답변은 가능하면 한국어로 작성해 주세요.".to_string())
        }
        OutputLanguagePreference::Auto => None,
    };

    if langs.is_empty() && app.is_none() && script_line.is_none() && output_language_line.is_none()
    {
        return None;
    }

    let mut lines = vec!["# 上下文".to_string()];
    if !langs.is_empty() {
        lines.push(format!(
            "用户的工作语言：{}。处理任何文本时请把这一前提带进考虑（识别专名、判定语气、决定写法）。",
            langs.join("、")
        ));
    }
    if let Some(name) = app {
        lines.push(format!(
            "当前前台应用：{name}。请按这个应用的常见沟通风格调整语气——例如邮件类 app 偏正式、聊天类 app 偏口语、IDE / 文档类 app 偏技术或结构化。\u{4E0D}主动加入与用户原意无关的客套话。"
        ));
    }
    if let Some(line) = script_line {
        lines.push(line);
    }
    if let Some(line) = output_language_line {
        lines.push(line);
    }
    Some(lines.join("\n"))
}

/// 把 polish 输入参数装配成 `(system_prompt, user_prompt)` 二元组。
///
/// 抽出来是为了让 OpenAI 兼容客户端 (本文件) 和谷歌原生 Gemini 客户端
/// (`llm_gemini.rs`) 共享同一套 prompt 装配规则——不再担心两路 LLM
/// 在 `system_prompt` 拼接顺序、context_premise 注入时机、
/// polish_context_instruction 追加条件上慢慢漂移。
pub(crate) fn compose_polish_prompts(
    raw_text: &str,
    _mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    front_app: Option<&str>,
    has_prior_turns: bool,
) -> (String, String) {
    let mut system_prompt = compose_system_prompt(style_system_prompt, hotwords);
    if let Some(premise) = context_premise(
        working_languages,
        chinese_script_preference,
        output_language_preference,
        front_app,
    ) {
        system_prompt = format!("{}\n\n{}", premise, system_prompt);
    }
    // issue #609 F-02：在 system prompt 末尾追加对抗式防御措辞，明确信封内文本是
    // 数据而非指令。纵深防御，非硬保证。
    system_prompt = format!(
        "{}\n\n{}",
        system_prompt,
        prompts::polish_injection_defense()
    );
    // 多轮上下文模式：把"上一轮的指令是什么、不要复读上一轮答案"明确写进
    // system prompt，配合 chat structure 让 LLM 自然不重复历史输出。
    if has_prior_turns {
        system_prompt = format!(
            "{}\n\n{}",
            system_prompt,
            prompts::polish_context_instruction()
        );
    }
    let user_prompt = prompts::user_prompt(raw_text);
    (system_prompt, user_prompt)
}

/// 翻译路径的 `(system_prompt, user_prompt)` 装配——和 polish 一样供两路 LLM 客户端共用。
/// 翻译模式以 `target_language` 为唯一输出语言约束，OutputLanguagePreference 在这里被
/// 强制设为 Auto 以避免 UI 偏好（如 ja）与 target_language（如 en）冲突。
pub(crate) fn assemble_polish_system_prompt(
    style_system_prompt: &str,
    hotwords: &[String],
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    front_app: Option<&str>,
    has_prior_turns: bool,
) -> PolishSystemPromptAssembly {
    let (effective_system_prompt, _) = compose_polish_prompts(
        "",
        PolishMode::Light,
        hotwords,
        style_system_prompt,
        working_languages,
        chinese_script_preference,
        output_language_preference,
        front_app,
        has_prior_turns,
    );
    let context_premise = context_premise(
        working_languages,
        chinese_script_preference,
        output_language_preference,
        front_app,
    )
    .unwrap_or_default();
    let hotword_block = compose_hotword_block_preview(hotwords);
    let history_instruction = if has_prior_turns {
        prompts::polish_context_instruction().to_string()
    } else {
        String::new()
    };
    let includes_hotword_block = !hotword_block.is_empty();
    let includes_context_premise = !context_premise.is_empty();
    PolishSystemPromptAssembly {
        context_premise,
        hotword_block,
        history_instruction,
        effective_system_prompt,
        includes_context_premise,
        includes_hotword_block,
        includes_history_instruction: has_prior_turns,
    }
}

pub(crate) fn compose_translate_prompts(
    raw_text: &str,
    target_language: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    front_app: Option<&str>,
) -> (String, String) {
    let mut system_prompt = prompts::translate_system_prompt(target_language);
    if let Some(premise) = context_premise(
        working_languages,
        chinese_script_preference,
        OutputLanguagePreference::Auto,
        front_app,
    ) {
        system_prompt = format!("{}\n\n{}", premise, system_prompt);
    }
    let user_prompt = prompts::user_prompt(raw_text);
    (system_prompt, user_prompt)
}

/// QA 划词问答的 system_prompt 装配。两路 LLM 客户端共用。
pub(crate) fn compose_qa_system_prompt(
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    front_app: Option<&str>,
) -> String {
    let mut system_prompt = prompts::qa_system_prompt();
    if let Some(premise) = context_premise(
        working_languages,
        chinese_script_preference,
        output_language_preference,
        front_app,
    ) {
        system_prompt = format!("{}\n\n{}", premise, system_prompt);
    }
    system_prompt
}

/// 构建「热词 + 错别字纠错」模块文本：agent-style 措辞，把模型当成接到一段 ASR 转写
/// 的写作助手，明确告诉它「输入可能有错别字，按这个列表 + 上下文修正」。
///
/// 内置 default prompt 里的 `{{HOTWORDS}}` 占位符被这段文本替换；用户自定义 prompt
/// 没占位符时 compose_system_prompt 兜底拼到末尾。
///
/// 这段文本 100% 对齐 compose_hotword_block_preview，让 Style Pack 设置页的预览跟
/// 实际发给 LLM 的 prompt 一致。
pub(super) fn build_hotword_block(hotwords: &[String]) -> String {
    let cleaned: Vec<String> = hotwords
        .iter()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();

    if cleaned.is_empty() {
        return "# 热词与纠错（系统内置）\n\
            你接到的转写来自 ASR，可能含错别字 / 同音误识别 / 形近词。\
            按上下文自动纠回正确字面：常见模式如「跟目录 / 根木鹿」→「根目录」、\
            「代码厂」→「代码仓」、「编一编」→「编译」、英文短词同音（如 VIP / ZIP）按上下文判断、\
            带次版本号产品名（GPT-5.6 不省略成 GPT-5）。\
            人名 / 品牌名 / 含义会变化的词原样保留，不强行改字。"
            .to_string();
    }

    let bullets = cleaned
        .iter()
        .map(|h| format!("- {}", h))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# 热词与纠错（系统内置）\n\
         你接到的转写来自 ASR，可能含错别字。用户希望以下写法在输出中保持准确；\
         当转写中出现这些词的同音或形近误识别时，优先按上述写法输出，不做无关词的机械替换：\n\
         {bullets}\n\
         \n\
         上面热词的纠偏指令优先于通用规则 2 的「原样保留」——当转写词是热词的同音 / 形近误识别\
         （例：转写出「VIP」而热词里有「ZIP」），就按热词写法输出，不要因为它看起来像英文专有名词\
         或中英混输而保留误识别结果。\n\
         \n\
         转写中其它 ASR 错别字按上下文自动纠回正确字面：常见模式如「跟目录 / 根木鹿」→「根目录」、\
         英文短词同音（如 VIP / ZIP）按上下文判断、带次版本号产品名（GPT-5.6 不省略成 GPT-5）。\
         人名 / 品牌名 / 含义会变化的词原样保留。",
        bullets = bullets
    )
}

/// 系统提示词组装：先把内置 default prompt 的 `{{HOTWORDS}}` 占位符替换为实际热词块；
/// 用户自定义 prompt 没占位符时 fallback 行为：
/// - hotwords 非空 → 末尾追加热词块（兼容历史 prompt 仍能拿到热词）
/// - hotwords 空 → 不附加任何东西（用户决定自己 prompt 的内容，不强行注入）
pub(super) fn compose_system_prompt(style_system_prompt: &str, hotwords: &[String]) -> String {
    let base = style_system_prompt.trim_end();
    if base.contains(crate::types::HOTWORDS_PLACEHOLDER) {
        let block = build_hotword_block(hotwords);
        return base.replace(crate::types::HOTWORDS_PLACEHOLDER, &block);
    }
    let has_hotwords = hotwords.iter().any(|h| !h.trim().is_empty());
    if !has_hotwords {
        return base.to_string();
    }
    format!("{}\n\n{}", base, build_hotword_block(hotwords))
}

pub(super) fn compose_hotword_block_preview(hotwords: &[String]) -> String {
    // Style Pack 设置页的预览 100% 跟 system prompt 用同一段文本，避免「设置里看到一段、
    // 实际发给 LLM 是另一段」的不一致。空热词时返回纯错别字纠错指南。
    build_hotword_block(hotwords)
}
