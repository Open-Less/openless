//! Bounded, local policy evaluation between ASR and LLM polish.
//!
//! The policy is intentionally small: it cannot perform I/O or call host
//! functions, and it can only select the polish path or override a short
//! allowlist of request fields. Errors fail open to the normal pipeline.

use std::fmt;

use rhai::{Dynamic, Engine, Map, Scope};

const MAX_SCRIPT_BYTES: usize = 32 * 1024;
const MAX_INPUT_TEXT_BYTES: usize = 128 * 1024;
const MAX_OPERATIONS: u64 = 50_000;
const MAX_CALL_LEVELS: usize = 16;
const MAX_STRING_BYTES: usize = 256 * 1024;
const MAX_COLLECTION_ITEMS: usize = 256;
const MAX_OUTPUT_TOKENS: u32 = 1_000_000;

/// A strategy configured for one captured dictation session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolishStrategyConfig {
    pub enabled: bool,
    pub script: String,
}

/// Metadata made available to the user policy after ASR completes.
#[derive(Debug, Clone, PartialEq)]
pub struct PolishStrategyInput {
    pub text: String,
    pub char_count: usize,
    pub audio_duration_ms: u64,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub asr_provider: String,
    pub asr_model: Option<String>,
}

/// A bounded subset of parameters that can be changed for one polish call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolishRequestOverrides {
    /// Temperature in thousandths, so the captured context remains Eq.
    pub temperature_milli: Option<u32>,
    pub max_tokens: Option<u32>,
    pub reasoning_effort: Option<ReasoningEffort>,
}

impl PolishRequestOverrides {
    pub fn temperature(&self) -> Option<f32> {
        self.temperature_milli.map(|value| value as f32 / 1_000.0)
    }

    pub fn is_empty(&self) -> bool {
        self.temperature_milli.is_none()
            && self.max_tokens.is_none()
            && self.reasoning_effort.is_none()
    }
}

/// Normalized reasoning values accepted by the policy API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
}

/// Result of evaluating a user strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolishStrategyDecision {
    Skip,
    Polish,
    Request(PolishRequestOverrides),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrategyError {
    Disabled,
    Empty,
    ScriptTooLarge,
    InputTooLarge,
    InvalidReturn,
    Runtime,
}

impl fmt::Display for StrategyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Disabled => "disabled",
            Self::Empty => "empty script",
            Self::ScriptTooLarge => "script exceeds the size limit",
            Self::InputTooLarge => "transcript exceeds the input limit",
            Self::InvalidReturn => "invalid strategy return",
            Self::Runtime => "script execution failed",
        })
    }
}

/// Evaluate a policy. Any error returns `None`, which means the caller should
/// retain the ordinary `uses_llm_polisher` and short-transcript decisions.
pub fn evaluate(
    config: &PolishStrategyConfig,
    input: &PolishStrategyInput,
) -> Option<PolishStrategyDecision> {
    match evaluate_inner(config, input) {
        Ok(decision) => Some(decision),
        Err(error) => {
            if !matches!(error, StrategyError::Disabled | StrategyError::Empty) {
                log::warn!("[polish-strategy] {error}; falling back to normal polish policy");
            }
            None
        }
    }
}

fn evaluate_inner(
    config: &PolishStrategyConfig,
    input: &PolishStrategyInput,
) -> Result<PolishStrategyDecision, StrategyError> {
    if !config.enabled {
        return Err(StrategyError::Disabled);
    }
    if config.script.trim().is_empty() {
        return Err(StrategyError::Empty);
    }
    if config.script.len() > MAX_SCRIPT_BYTES {
        return Err(StrategyError::ScriptTooLarge);
    }
    if input.text.len() > MAX_INPUT_TEXT_BYTES {
        return Err(StrategyError::InputTooLarge);
    }

    let mut engine = Engine::new();
    engine.set_module_resolver(rhai::module_resolvers::StaticModuleResolver::new());
    engine.disable_symbol("import");
    engine.on_print(|_| {});
    engine.on_debug(|_, _, _| {});
    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_max_string_size(MAX_STRING_BYTES);
    engine.set_max_array_size(MAX_COLLECTION_ITEMS);
    engine.set_max_map_size(MAX_COLLECTION_ITEMS);

    let mut scope = Scope::new();
    scope.push("input", input_map(input));
    let expression = format!("{}\nstrategy(input)", config.script);
    let result = engine
        .eval_with_scope::<Dynamic>(&mut scope, &expression)
        .map_err(|_| StrategyError::Runtime)?;
    parse_decision(result)
}

fn input_map(input: &PolishStrategyInput) -> Map {
    let mut map = Map::new();
    map.insert("text".into(), input.text.clone().into());
    map.insert("charCount".into(), (input.char_count as i64).into());
    map.insert(
        "audioDurationMs".into(),
        (input.audio_duration_ms as i64).into(),
    );
    map.insert(
        "language".into(),
        input
            .language
            .clone()
            .map(Dynamic::from)
            .unwrap_or(Dynamic::UNIT),
    );
    map.insert(
        "confidence".into(),
        input
            .confidence
            .map(|value| Dynamic::from(value as f64))
            .unwrap_or(Dynamic::UNIT),
    );
    map.insert("asrProvider".into(), input.asr_provider.clone().into());
    map.insert(
        "asrModel".into(),
        input
            .asr_model
            .clone()
            .map(Dynamic::from)
            .unwrap_or(Dynamic::UNIT),
    );
    map
}

fn parse_decision(result: Dynamic) -> Result<PolishStrategyDecision, StrategyError> {
    let map = result
        .try_cast::<Map>()
        .ok_or(StrategyError::InvalidReturn)?;
    if map.keys().any(|key| {
        !matches!(
            key.as_str(),
            "action" | "temperature" | "maxTokens" | "reasoningEffort"
        )
    }) {
        return Err(StrategyError::InvalidReturn);
    }
    let action = map
        .get("action")
        .and_then(|value| value.clone().try_cast::<String>())
        .ok_or(StrategyError::InvalidReturn)?;
    let overrides = parse_overrides(&map)?;
    match action.trim() {
        "skip" if overrides.is_empty() => Ok(PolishStrategyDecision::Skip),
        "polish" if overrides.is_empty() => Ok(PolishStrategyDecision::Polish),
        "request" => Ok(PolishStrategyDecision::Request(overrides)),
        _ => Err(StrategyError::InvalidReturn),
    }
}

fn parse_overrides(map: &Map) -> Result<PolishRequestOverrides, StrategyError> {
    let temperature_milli = match map.get("temperature") {
        None => None,
        Some(value) => {
            let value = numeric_f64(value).ok_or(StrategyError::InvalidReturn)?;
            if !value.is_finite() || !(0.0..=2.0).contains(&value) {
                return Err(StrategyError::InvalidReturn);
            }
            Some((value * 1_000.0).round() as u32)
        }
    };
    let max_tokens = match map.get("maxTokens") {
        None => None,
        Some(value) => {
            let value = value
                .clone()
                .try_cast::<i64>()
                .ok_or(StrategyError::InvalidReturn)?;
            if !(1..=i64::from(MAX_OUTPUT_TOKENS)).contains(&value) {
                return Err(StrategyError::InvalidReturn);
            }
            Some(value as u32)
        }
    };
    let reasoning_effort = match map.get("reasoningEffort") {
        None => None,
        Some(value) => {
            let value = value
                .clone()
                .try_cast::<String>()
                .ok_or(StrategyError::InvalidReturn)?;
            Some(match value.trim() {
                "none" => ReasoningEffort::None,
                "low" => ReasoningEffort::Low,
                "medium" => ReasoningEffort::Medium,
                "high" => ReasoningEffort::High,
                _ => return Err(StrategyError::InvalidReturn),
            })
        }
    };
    Ok(PolishRequestOverrides {
        temperature_milli,
        max_tokens,
        reasoning_effort,
    })
}

fn numeric_f64(value: &Dynamic) -> Option<f64> {
    value
        .clone()
        .try_cast::<f64>()
        .or_else(|| value.clone().try_cast::<i64>().map(|value| value as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> PolishStrategyInput {
        PolishStrategyInput {
            text: text.to_string(),
            char_count: text.chars().filter(|c| !c.is_whitespace()).count(),
            audio_duration_ms: 1_250,
            language: Some("en".into()),
            confidence: Some(0.91),
            asr_provider: "test-asr".into(),
            asr_model: Some("tiny".into()),
        }
    }

    fn config(script: &str) -> PolishStrategyConfig {
        PolishStrategyConfig {
            enabled: true,
            script: script.into(),
        }
    }

    #[test]
    fn evaluates_skip_and_exposes_input() {
        let decision = evaluate(
            &config(
                r#"fn strategy(input) { if input["charCount"] <= 3 && input["language"] == "en" { #{ action: "skip" } } else { #{ action: "polish" } } }"#,
            ),
            &input("hi "),
        );
        assert_eq!(decision, Some(PolishStrategyDecision::Skip));
    }

    #[test]
    fn evaluates_request_overrides() {
        let decision = evaluate(
            &config(
                r#"fn strategy(input) { #{ action: "request", temperature: 0.4, maxTokens: 400, reasoningEffort: "low" } }"#,
            ),
            &input("a longer transcript"),
        );
        assert_eq!(
            decision,
            Some(PolishStrategyDecision::Request(PolishRequestOverrides {
                temperature_milli: Some(400),
                max_tokens: Some(400),
                reasoning_effort: Some(ReasoningEffort::Low),
            }))
        );
    }

    #[test]
    fn malformed_or_disabled_scripts_fail_open() {
        assert_eq!(
            evaluate(&PolishStrategyConfig::default(), &input("hello")),
            None
        );
        assert_eq!(
            evaluate(
                &config(r#"fn strategy(input) { #{ action: "unknown" } }"#),
                &input("hello")
            ),
            None
        );
        assert_eq!(
            evaluate(
                &config(r#"fn strategy(input) { #{ action: "polish", unsupported: true } }"#),
                &input("hello")
            ),
            None
        );
    }

    #[test]
    fn missing_values_are_unit_and_can_be_checked() {
        let mut value = input("hello");
        value.language = None;
        value.confidence = None;
        value.asr_model = None;
        let decision = evaluate(
            &config(
                r#"fn strategy(input) { if input["language"] == () && input["confidence"] == () { #{ action: "skip" } } else { #{ action: "polish" } } }"#,
            ),
            &value,
        );
        assert_eq!(decision, Some(PolishStrategyDecision::Skip));
    }

    #[test]
    fn filesystem_imports_fail_open_even_when_the_module_exists() {
        let path =
            std::env::temp_dir().join(format!("openless-strategy-{}.rhai", uuid::Uuid::new_v4()));
        std::fs::write(&path, "fn decision() { #{ action: \"skip\" } }").unwrap();
        let module = serde_json::to_string(path.with_extension("").to_str().unwrap()).unwrap();
        let direct =
            format!("import {module} as external; fn strategy(input) {{ external::decision() }}");
        let dynamic = format!(
            "fn strategy(input) {{ eval({}); #{{ action: \"skip\" }} }}",
            serde_json::to_string(&format!("import {module} as external;")).unwrap()
        );
        let direct_result = evaluate(&config(&direct), &input("hello"));
        let dynamic_result = evaluate(&config(&dynamic), &input("hello"));
        std::fs::remove_file(path).unwrap();
        assert_eq!(direct_result, None);
        assert_eq!(dynamic_result, None);
    }

    #[test]
    fn script_size_limit_fails_open() {
        let script = "x".repeat(MAX_SCRIPT_BYTES + 1);
        assert_eq!(evaluate(&config(&script), &input("hello")), None);
    }
}
