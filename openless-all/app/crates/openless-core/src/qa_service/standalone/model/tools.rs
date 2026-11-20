//! Provider-native function calling. Search output is returned as a tool result, not prompt text.

use serde_json::{json, Value};

use crate::ask::{AskModelSearch, AskModelToolCall};
use crate::llm_protocol::LlmRequestFormat;
use crate::polish::LLMError;

const FUNCTION: &str = "web_search";
const DESCRIPTION: &str = "Search the public web when current or external facts are needed. Only use a short public query. Do not include private material, identifiers, account data, passwords, or quoted content in the query.";
const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_RESUME_BYTES: usize = 32 * 1024;

fn invalid() -> LLMError {
    LLMError::ParseError("askInvalidToolCall".into())
}

pub(super) fn configure(body: &mut Value, format: LlmRequestFormat, search: &AskModelSearch) {
    match (format, search) {
        (LlmRequestFormat::ChatCompletions, AskModelSearch::Available) => {
            body["tools"] = json!([{"type":"function","function":{
                "name":FUNCTION,"description":DESCRIPTION,"parameters":schema()
            }}]);
            body["tool_choice"] = json!("auto");
        }
        (LlmRequestFormat::Responses, AskModelSearch::Available) => {
            body["store"] = json!(false);
            body["include"] = json!(["reasoning.encrypted_content"]);
            body["tools"] = json!([{"type":"function","name":FUNCTION,"description":DESCRIPTION,
                "parameters":schema(), "strict":true}]);
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(false);
        }
        (LlmRequestFormat::ChatCompletions, AskModelSearch::Result { call, output }) => {
            if let Some(messages) = body["messages"].as_array_mut() {
                messages.extend(call.resume.iter().cloned());
                messages.push(json!({"role":"tool","tool_call_id":call.id,"content":output}));
            }
        }
        (LlmRequestFormat::Responses, AskModelSearch::Result { call, output }) => {
            body["store"] = json!(false);
            if let Some(input) = body["input"].as_array_mut() {
                input.extend(call.resume.iter().cloned());
                input
                    .push(json!({"type":"function_call_output","call_id":call.id,"output":output}));
            }
        }
        (LlmRequestFormat::Messages, AskModelSearch::Available) => {
            body["tools"] =
                json!([{"name":FUNCTION,"description":DESCRIPTION,"input_schema":schema()}]);
            body["tool_choice"] = json!({"type":"auto","disable_parallel_tool_use":true});
        }
        (LlmRequestFormat::Messages, AskModelSearch::Result { call, output }) => {
            if let Some(messages) = body["messages"].as_array_mut() {
                messages.extend(call.resume.iter().cloned());
                messages.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":call.id,"content":output}]}));
            }
        }
        _ => {}
    }
}

pub(super) fn configure_gemini(body: &mut Value, search: &AskModelSearch) {
    match search {
        AskModelSearch::Available => {
            body["tools"] = json!([{"functionDeclarations":[{"name":FUNCTION,"description":DESCRIPTION,"parametersJsonSchema":schema()}]}]);
        }
        AskModelSearch::Result { call, output } => {
            if let Some(contents) = body["contents"].as_array_mut() {
                contents.extend(call.resume.iter().cloned());
                let mut response = json!({"name":FUNCTION,"response":{"result":output}});
                if call.id != FUNCTION {
                    response["id"] = json!(call.id);
                }
                contents.push(json!({"role":"user","parts":[{"functionResponse":response}]}));
            }
        }
        AskModelSearch::Disabled => {}
    }
}

fn schema() -> Value {
    json!({"type":"object", "properties":{"query":{"type":"string"}}, "required":["query"], "additionalProperties":false})
}

pub(super) struct SearchStream {
    format: LlmRequestFormat,
    gemini: bool,
    enabled: bool,
    id: String,
    name: String,
    args: String,
    reasoning: String,
    content: String,
    blocks: std::collections::BTreeMap<u64, Value>,
    call: Option<AskModelToolCall>,
}

impl SearchStream {
    pub(super) fn new(format: LlmRequestFormat, gemini: bool, enabled: bool) -> Self {
        Self {
            format,
            gemini,
            enabled,
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
        if !self.enabled {
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
                            item["call_id"].as_str(),
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
                        func["id"].as_str().or(Some(FUNCTION)),
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
    id: Option<&str>,
    name: Option<&str>,
    args: &str,
    resume: Vec<Value>,
) -> Result<AskModelToolCall, LLMError> {
    let id = id
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or_else(invalid)?;
    if name != Some(FUNCTION)
        || args.len() > MAX_ARGUMENT_BYTES
        || json!(&resume).to_string().len() > MAX_RESUME_BYTES
    {
        return Err(invalid());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Arguments {
        query: String,
    }
    let args: Arguments = serde_json::from_str(args).map_err(|_| invalid())?;
    let query = args.query.trim();
    if query.is_empty()
        || query.chars().count() > crate::ask_search::MAX_PUBLIC_QUERY_CHARS
        || query.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(AskModelToolCall {
        query: query.to_owned(),
        id: id.to_owned(),
        resume,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_protocol::TextEventStream;

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
            let mut observer = SearchStream::new(format, gemini, true);
            for frame in frames {
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
            assert_eq!(call.query, "public facts");
            let mut body = json!({key:[]});
            let resume = AskModelSearch::Result {
                call,
                output: "tool evidence".into(),
            };
            if gemini {
                configure_gemini(&mut body, &resume);
            } else {
                configure(&mut body, format, &resume);
            }
            assert_eq!(body[key].as_array().unwrap().len(), 2);
            let last = body[key][1].to_string();
            assert!(last.contains("call-a") && last.contains("tool evidence"));
        }
    }
}
