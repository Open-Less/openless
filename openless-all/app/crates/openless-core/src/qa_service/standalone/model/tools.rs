//! Provider-native function calls and turn-scoped results.

use serde_json::{json, Value};

use crate::ask::{AskModelToolCall, AskModelTools, AskToolCall, AskToolResult, AskToolSet};
use crate::llm_protocol::LlmRequestFormat;
use crate::polish::LLMError;

const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_RESUME_BYTES: usize = 32 * 1024;

fn invalid() -> LLMError {
    LLMError::ParseError("askInvalidToolCall".into())
}

fn definitions(available: AskToolSet) -> impl Iterator<Item = (&'static str, &'static str, Value)> {
    let schema = |field: &str| {
        let mut properties = serde_json::Map::new();
        properties.insert(field.to_owned(), json!({"type":"string"}));
        json!({"type":"object", "properties":properties, "required":[field], "additionalProperties":false})
    };
    [
        available.resolve_intent.then(|| (
            "resolve_intent",
            "Resolve the user's final destination before answering. This decision does not authorize external effects.",
            json!({
                "type": "object",
                "properties": {"route": {
                    "type": "string", "enum": ["answer", "selectionAnswer", "write", "edit", "clarify"]
                }},
                "required": ["route"],
                "additionalProperties": false
            }),
        )),
        available.search.then(|| (
            "web_search",
            "Search for public facts, especially time-sensitive or changeable information. Use only a short public query. Never include private material, identifiers, account data, or quotes.",
            schema("query"),
        )),
        available.calculator.then(|| (
            "calculator",
            "Evaluate exact arithmetic locally. Use decimal or scientific notation, parentheses, + - * / and ^ with integer exponents. Write percentages as /100. Returns an exact integer or fraction and up to 50 decimal places; no functions, variables, units or code.",
            schema("expression"),
        )),
    ].into_iter().flatten()
}

pub(super) fn configure(body: &mut Value, format: LlmRequestFormat, tools: &AskModelTools) {
    if tools.available.any() {
        body["tools"] = Value::Array(
            definitions(tools.available)
                .map(|(name, description, schema)| match format {
                    LlmRequestFormat::ChatCompletions => json!({"type":"function","function":{
                        "name":name,"description":description,"parameters":schema
                    }}),
                    LlmRequestFormat::Responses => json!({"type":"function","name":name,
                    "description":description,"parameters":schema,"strict":true}),
                    LlmRequestFormat::Messages => {
                        json!({"name":name,"description":description,"input_schema":schema})
                    }
                })
                .collect(),
        );
        body["tool_choice"] = match (format, tools.available.resolve_intent) {
            (LlmRequestFormat::ChatCompletions, true) => {
                json!({"type":"function","function":{"name":"resolve_intent"}})
            }
            (LlmRequestFormat::Responses, true) => {
                json!({"type":"function","name":"resolve_intent"})
            }
            (LlmRequestFormat::Messages, true) => {
                json!({"type":"tool","name":"resolve_intent","disable_parallel_tool_use":true})
            }
            (LlmRequestFormat::Messages, false) => {
                json!({"type":"auto","disable_parallel_tool_use":true})
            }
            _ => json!("auto"),
        };
        if format == LlmRequestFormat::Responses {
            body["include"] = json!(["reasoning.encrypted_content"]);
            body["parallel_tool_calls"] = json!(false);
        }
    }
    if format == LlmRequestFormat::Responses
        && (tools.available.any() || !tools.results.is_empty() || tools.intent.is_some())
    {
        body["store"] = json!(false);
    }
    let key = if format == LlmRequestFormat::Responses {
        "input"
    } else {
        "messages"
    };
    if let Some(messages) = body[key].as_array_mut() {
        if let Some(intent) = &tools.intent {
            let offset = if format == LlmRequestFormat::Messages {
                1
            } else {
                2
            };
            messages.splice(offset..offset, compatible_result(format, &intent.result));
        }
        for result in &tools.results {
            messages.extend(compatible_result(format, result));
        }
    }
}

fn compatible_result(
    format: LlmRequestFormat,
    AskToolResult { call, output }: &AskToolResult,
) -> Vec<Value> {
    let mut messages = call.resume.clone();
    messages.push(match format {
        LlmRequestFormat::ChatCompletions => json!({"role":"tool","tool_call_id":call.id,"content":output}),
        LlmRequestFormat::Responses => json!({"type":"function_call_output","call_id":call.id,"output":output}),
        LlmRequestFormat::Messages => json!({"role":"user","content":[{"type":"tool_result","tool_use_id":call.id,"content":output}]}),
    });
    messages
}

pub(super) fn configure_gemini(body: &mut Value, tools: &AskModelTools) {
    if tools.available.any() {
        let declarations: Vec<_> = definitions(tools.available).map(|(name, description, schema)| {
            json!({"name":name,"description":description,"parametersJsonSchema":schema})
        }).collect();
        body["tools"] = json!([{"functionDeclarations":declarations}]);
        if tools.available.resolve_intent {
            body["toolConfig"] = json!({
                "functionCallingConfig": {"mode":"ANY","allowedFunctionNames":["resolve_intent"]}
            });
        }
    }
    if let Some(contents) = body["contents"].as_array_mut() {
        if let Some(intent) = &tools.intent {
            contents.splice(1..1, gemini_result(&intent.result));
        }
        for result in &tools.results {
            contents.extend(gemini_result(result));
        }
    }
}

fn gemini_result(AskToolResult { call, output }: &AskToolResult) -> Vec<Value> {
    let mut contents = call.resume.clone();
    let mut response = json!({"name":call.tool.name(),"response":{"result":output}});
    if let Some(id) = &call.id {
        response["id"] = json!(id);
    }
    contents.push(json!({"role":"user","parts":[{"functionResponse":response}]}));
    contents
}

pub(super) struct ToolStream {
    format: LlmRequestFormat,
    gemini: bool,
    available: AskToolSet,
    id: String,
    name: String,
    args: String,
    reasoning: String,
    content: String,
    blocks: std::collections::BTreeMap<u64, Value>,
    call: Option<AskModelToolCall>,
}

impl ToolStream {
    pub(super) fn new(format: LlmRequestFormat, gemini: bool, available: AskToolSet) -> Self {
        Self {
            format,
            gemini,
            available,
            id: String::new(),
            name: String::new(),
            args: String::new(),
            reasoning: String::new(),
            content: String::new(),
            blocks: Default::default(),
            call: None,
        }
    }

    pub(super) fn observe(&mut self, value: &Value, kind: &str) -> Result<bool, LLMError> {
        if !self.available.any() {
            return Ok(false);
        }
        if self.gemini {
            return self.gemini(value);
        }
        match self.format {
            LlmRequestFormat::ChatCompletions => self.chat(&value["choices"][0]),
            LlmRequestFormat::Messages => self.messages(value, kind),
            LlmRequestFormat::Responses if kind == "response.completed" => {
                let output = &value["response"]["output"];
                if value["response"]["status"] == "completed" {
                    let items = output.as_array().map(Vec::as_slice).unwrap_or_default();
                    if self.available.resolve_intent
                        && items.iter().any(|item| item["type"] == "message")
                    {
                        return Err(invalid());
                    }
                    let mut calls = items.iter().filter(|item| item["type"] == "function_call");
                    if let Some(item) = calls.next() {
                        if calls.next().is_some() || self.call.is_some() {
                            return Err(invalid());
                        }
                        let args = item["arguments"].as_str().ok_or_else(invalid)?;
                        let resume = items
                            .iter()
                            .filter(|item| {
                                matches!(item["type"].as_str(), Some("reasoning" | "function_call"))
                            })
                            .cloned()
                            .collect();
                        self.call = Some(finish_call(
                            self.available,
                            Some(item["call_id"].as_str().ok_or_else(invalid)?),
                            item["name"].as_str(),
                            args,
                            resume,
                        )?);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn gemini(&mut self, value: &Value) -> Result<bool, LLMError> {
        let candidate = &value["candidates"][0];
        if let Some(parts) = candidate["content"]["parts"].as_array() {
            for part in parts {
                if let Some(func) = part.get("functionCall") {
                    if self.call.is_some() {
                        return Err(invalid());
                    }
                    self.call = Some(finish_call(
                        self.available,
                        func["id"].as_str(),
                        func["name"].as_str(),
                        &func["args"].to_string(),
                        vec![json!({"role":"model","parts":[part]})],
                    )?);
                }
            }
        }
        Ok(false)
    }

    fn messages(&mut self, value: &Value, kind: &str) -> Result<bool, LLMError> {
        match kind {
            "content_block_start" => {
                let block = &value["content_block"];
                if matches!(
                    block["type"].as_str(),
                    Some("tool_use" | "thinking" | "redacted_thinking")
                ) {
                    let index = value["index"].as_u64().ok_or_else(invalid)?;
                    if self.blocks.len() >= 6 || self.blocks.insert(index, block.clone()).is_some()
                    {
                        return Err(invalid());
                    }
                }
            }
            "content_block_delta" => {
                if let Some(block) = value["index"]
                    .as_u64()
                    .and_then(|index| self.blocks.get_mut(&index))
                {
                    let delta = &value["delta"];
                    match delta["type"].as_str() {
                        Some("input_json_delta") => {
                            let chunk = delta["partial_json"].as_str().ok_or_else(invalid)?;
                            if self.args.len().saturating_add(chunk.len()) > MAX_ARGUMENT_BYTES {
                                return Err(invalid());
                            }
                            self.args.push_str(chunk);
                        }
                        Some("thinking_delta" | "signature_delta") => {
                            let field = if delta["type"] == "thinking_delta" {
                                "thinking"
                            } else {
                                "signature"
                            };
                            let mut text = block[field].as_str().unwrap_or_default().to_owned();
                            let chunk = delta[field].as_str().ok_or_else(invalid)?;
                            if text.len().saturating_add(chunk.len()) > MAX_RESUME_BYTES {
                                return Err(invalid());
                            }
                            text.push_str(chunk);
                            block[field] = json!(text);
                        }
                        _ => {}
                    }
                }
            }
            "message_delta" if value["delta"]["stop_reason"] == "tool_use" => {
                let mut blocks = std::mem::take(&mut self.blocks);
                let mut candidates = blocks
                    .values_mut()
                    .filter(|block| block["type"] == "tool_use");
                let block = candidates.next().ok_or_else(invalid)?;
                if candidates.next().is_some() || self.call.is_some() {
                    return Err(invalid());
                }
                if !self.args.is_empty() {
                    block["input"] = serde_json::from_str(&self.args).map_err(|_| invalid())?;
                }
                let id = block["id"].as_str().ok_or_else(invalid)?.to_owned();
                let name = block["name"].as_str().ok_or_else(invalid)?.to_owned();
                let args = block["input"].to_string();
                self.call = Some(finish_call(
                    self.available,
                    Some(&id),
                    Some(&name),
                    &args,
                    vec![
                        json!({"role":"assistant","content":blocks.into_values().collect::<Vec<_>>()}),
                    ],
                )?);
                return Ok(true);
            }
            "message_delta"
                if self
                    .blocks
                    .values()
                    .any(|block| block["type"] == "tool_use") =>
            {
                return Err(invalid())
            }
            _ => {}
        }
        Ok(false)
    }

    fn chat(&mut self, choice: &Value) -> Result<bool, LLMError> {
        let delta = &choice["delta"];
        // Preserve bounded provider reasoning when the provider requires round-tripping it.
        append_bounded(
            &mut self.reasoning,
            delta["reasoning_content"].as_str(),
            MAX_RESUME_BYTES,
        );
        append_bounded(
            &mut self.content,
            delta["content"].as_str(),
            MAX_RESUME_BYTES,
        );
        if let Some(calls) = delta["tool_calls"].as_array() {
            for call in calls {
                if call["index"].as_u64() != Some(0) || self.call.is_some() {
                    return Err(invalid());
                }
                if let Some(id) = call["id"].as_str() {
                    if id.len() > 256 || (!self.id.is_empty() && self.id != id) {
                        return Err(invalid());
                    }
                    self.id = id.to_owned();
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    if name.len() > 64 || (!self.name.is_empty() && self.name != name) {
                        return Err(invalid());
                    }
                    self.name = name.to_owned();
                }
                if let Some(args) = call["function"]["arguments"].as_str() {
                    if self.args.len().saturating_add(args.len()) > MAX_ARGUMENT_BYTES {
                        return Err(invalid());
                    }
                    self.args.push_str(args);
                }
            }
        }
        if choice["finish_reason"] != "tool_calls" {
            if choice["finish_reason"] == "stop" && (!self.id.is_empty() || !self.args.is_empty()) {
                return Err(invalid());
            }
            return Ok(false);
        }
        if self.call.is_some() {
            return Err(invalid());
        }
        let mut assistant = json!({"role":"assistant","content":self.content,
            "tool_calls":[{"id":self.id,"type":"function","function":{"name":self.name,"arguments":self.args}}]});
        if !self.reasoning.is_empty() {
            assistant["reasoning_content"] = json!(self.reasoning);
        }
        self.call = Some(finish_call(
            self.available,
            Some(&self.id),
            Some(&self.name),
            &self.args,
            vec![assistant],
        )?);
        Ok(true)
    }

    pub(super) fn take(self) -> Option<AskModelToolCall> {
        self.call
    }
}

fn append_bounded(current: &mut String, chunk: Option<&str>, max: usize) {
    if let Some(chunk) = chunk {
        if current.len().saturating_add(chunk.len()) <= max {
            current.push_str(chunk);
        }
    }
}

fn finish_call(
    available: AskToolSet,
    id: Option<&str>,
    name: Option<&str>,
    args: &str,
    resume: Vec<Value>,
) -> Result<AskModelToolCall, LLMError> {
    if id.is_some_and(|id| id.is_empty() || id.len() > 256)
        || args.len() > MAX_ARGUMENT_BYTES
        || json!(&resume).to_string().len() > MAX_RESUME_BYTES
    {
        return Err(invalid());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Query {
        query: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Expression {
        expression: String,
    }
    let tool = match name {
        Some("web_search") if available.search => {
            let Query { query } = serde_json::from_str(args).map_err(|_| invalid())?;
            let query = query.trim();
            if query.is_empty()
                || query.chars().count() > crate::ask_search::MAX_PUBLIC_QUERY_CHARS
                || query.chars().any(char::is_control)
            {
                return Err(invalid());
            }
            AskToolCall::WebSearch {
                public_query: query.to_owned(),
            }
        }
        Some("calculator") if available.calculator => {
            let Expression { expression } = serde_json::from_str(args).map_err(|_| invalid())?;
            AskToolCall::Calculate { expression }
        }
        Some("resolve_intent") if available.resolve_intent => {
            let decision: super::super::entry::Decision =
                serde_json::from_str(args).map_err(|_| invalid())?;
            AskToolCall::ResolveIntent {
                route: decision.route,
            }
        }
        _ => return Err(invalid()),
    };
    Ok(AskModelToolCall {
        tool,
        id: id.map(str::to_owned),
        resume,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ask::{AskIntentContinuation, AskModelMessage, AskModelRole, AskRoute};
    use crate::llm_protocol::TextEventStream;

    #[test]
    fn required_intent_call_resumes_before_private_generation_material_on_each_protocol() {
        let cases = [
            (
                LlmRequestFormat::ChatCompletions,
                false,
                "messages",
                2,
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"intent-1","function":{"name":"resolve_intent","arguments":"{\"route\":\"edit\"}"}}]},"finish_reason":"tool_calls"}]}),
            ),
            (
                LlmRequestFormat::Responses,
                false,
                "input",
                2,
                json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"intent-1","name":"resolve_intent","arguments":"{\"route\":\"edit\"}"}]}}),
            ),
            (
                LlmRequestFormat::Messages,
                false,
                "messages",
                1,
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"intent-1","name":"resolve_intent","input":{"route":"edit"}}}),
            ),
            (
                LlmRequestFormat::ChatCompletions,
                true,
                "contents",
                1,
                json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"intent-1","name":"resolve_intent","args":{"route":"edit"}},"thoughtSignature":"intent-signature"}]},"finishReason":"STOP"}]}),
            ),
        ];
        let available = AskToolSet {
            resolve_intent: true,
            ..Default::default()
        };
        let required = AskModelTools {
            available,
            ..Default::default()
        };
        for (format, gemini, key, offset, frame) in cases {
            let configure_body = |body: &mut Value, tools: &AskModelTools| {
                if gemini {
                    configure_gemini(body, tools);
                } else {
                    configure(body, format, tools);
                }
            };
            let mut body = json!({key: vec![json!("original sanitized input"); offset]});
            configure_body(&mut body, &required);
            let forced = if gemini {
                &body["toolConfig"]
            } else {
                &body["tool_choice"]
            };
            assert!(forced.to_string().contains("resolve_intent"));
            let mut observer = ToolStream::new(format, gemini, available);
            observer
                .observe(&frame, frame["type"].as_str().unwrap_or_default())
                .unwrap();
            if format == LlmRequestFormat::Messages {
                observer
                    .observe(
                        &json!({"delta":{"stop_reason":"tool_use"}}),
                        "message_delta",
                    )
                    .unwrap();
            }
            let call = observer.take().unwrap();
            assert!(matches!(
                call.tool,
                AskToolCall::ResolveIntent {
                    route: AskRoute::Edit
                }
            ));
            body = json!({key: vec![json!("original sanitized input"); offset]});
            body[key]
                .as_array_mut()
                .unwrap()
                .push(json!("private material"));
            configure_body(
                &mut body,
                &AskModelTools {
                    intent: Some(AskIntentContinuation {
                        input: AskModelMessage {
                            role: AskModelRole::User,
                            parts: Vec::new(),
                        },
                        result: AskToolResult {
                            call,
                            output: r#"{"route":"edit"}"#.into(),
                        },
                    }),
                    ..Default::default()
                },
            );
            assert!(body.get("tool_choice").is_none() && body.get("toolConfig").is_none());
            assert_eq!(body[key].as_array().unwrap().len(), offset + 3);
            assert!(body[key][offset].to_string().contains("intent-1"));
            assert!(body[key][offset + 1].to_string().contains("intent-1"));
            assert_eq!(body[key][offset + 2], "private material");
            if gemini {
                assert!(body[key][offset].to_string().contains("intent-signature"));
            }
        }
        for arguments in [
            r#"{"route":"replaceAll"}"#,
            r#"{"route":"edit","text":"unsafe"}"#,
        ] {
            assert!(finish_call(
                available,
                Some("intent-1"),
                Some("resolve_intent"),
                arguments,
                Vec::new()
            )
            .is_err());
        }
    }

    #[test]
    fn native_tool_calls_are_resumed_with_protocol_specific_results() {
        let cases: [(LlmRequestFormat, bool, Vec<Value>, &str); 4] = [
            (
                LlmRequestFormat::ChatCompletions,
                false,
                vec![
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-a","function":{"name":"web_search","arguments":"{\"query\":\""}}]}}]}),
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"public facts\"}"}}]},"finish_reason":"tool_calls"}]}),
                ],
                "messages",
            ),
            (
                LlmRequestFormat::Responses,
                false,
                vec![
                    json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"call-a","name":"web_search","arguments":"{\"query\":\"public facts\"}"}]}}),
                ],
                "input",
            ),
            (
                LlmRequestFormat::Messages,
                false,
                vec![
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call-a","name":"web_search","input":{"query":"public facts"}}}),
                    json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
                    json!({"type":"message_stop"}),
                ],
                "messages",
            ),
            (
                LlmRequestFormat::ChatCompletions,
                true,
                vec![
                    json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"call-a","name":"web_search","args":{"query":"public facts"}},"thoughtSignature":"signature"}]},"finishReason":"STOP"}]}),
                ],
                "contents",
            ),
        ];
        for (format, gemini, frames, key) in cases {
            let mut parser = TextEventStream::new(format).for_ask(gemini);
            let mut observer = ToolStream::new(
                format,
                gemini,
                AskToolSet {
                    search: true,
                    calculator: true,
                    ..Default::default()
                },
            );
            for frame in &frames {
                parser
                    .push(format!("data: {frame}\n\n").as_bytes())
                    .unwrap();
            }
            if format == LlmRequestFormat::ChatCompletions && !gemini {
                parser.push(b"data: [DONE]\n\n").unwrap();
            }
            while parser
                .next_with_observer(|value, kind| observer.observe(value, kind))
                .unwrap()
                .is_some()
            {}
            parser.finish().unwrap();
            let call = observer.take().unwrap();
            assert!(
                matches!(&call.tool, AskToolCall::WebSearch { public_query } if public_query == "public facts")
            );
            let mut body = json!({key:[]});
            let resume = AskModelTools {
                available: AskToolSet {
                    search: false,
                    calculator: true,
                    ..Default::default()
                },
                results: vec![AskToolResult {
                    call,
                    output: "tool evidence".into(),
                }],
                ..Default::default()
            };
            if gemini {
                configure_gemini(&mut body, &resume);
            } else {
                configure(&mut body, format, &resume);
            }
            assert_eq!(body[key].as_array().unwrap().len(), 2);
            let last = body[key][1].to_string();
            assert!(last.contains("call-a") && last.contains("tool evidence"));
            let mut calculator_only = ToolStream::new(format, gemini, resume.available);
            let mut parser = TextEventStream::new(format).for_ask(gemini);
            for frame in &frames {
                let frame = frame
                    .to_string()
                    .replace("web_search", "calculator")
                    .replace("query", "expression")
                    .replace("public facts", "0.1 + 0.2");
                parser
                    .push(format!("data: {frame}\n\n").as_bytes())
                    .unwrap();
            }
            if format == LlmRequestFormat::ChatCompletions && !gemini {
                parser.push(b"data: [DONE]\n\n").unwrap();
            }
            while parser
                .next_with_observer(|value, kind| calculator_only.observe(value, kind))
                .unwrap()
                .is_some()
            {}
            parser.finish().unwrap();
            let call = calculator_only.take().unwrap();
            assert!(
                matches!(&call.tool, AskToolCall::Calculate { expression } if expression == "0.1 + 0.2")
            );
            let mut body = json!({key:[]});
            let mut end = resume;
            end.available = AskToolSet::default();
            end.results.push(AskToolResult {
                call,
                output: "0.3".into(),
            });
            if gemini {
                configure_gemini(&mut body, &end);
                assert_eq!(
                    body[key][3]["parts"][0]["functionResponse"]["name"],
                    "calculator"
                );
            } else {
                configure(&mut body, format, &end);
            }
            assert_eq!(body[key].as_array().unwrap().len(), 4);
            assert!(body[key][3].to_string().contains("0.3"));
            assert!(body.get("tools").is_none());
        }
    }
}
