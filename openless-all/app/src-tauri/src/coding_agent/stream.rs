//! 解析 `claude -p --output-format stream-json` 的逐行 JSON 输出。
//!
//! 关注的几类行（其余忽略）：
//! - `stream_event` → `content_block_delta` → `text_delta`：逐字增量。
//! - `assistant` 消息里的 `tool_use` 块：工具调用提示。
//! - `result`：终局，带 `result` 文本、`total_cost_usd`、`duration_ms`、`is_error`。

/// 转发给前端的 agent 事件。`kind` 作为 tag，前端按 `kind` 分发。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodingAgentEvent {
    /// 进程已启动。
    Started { session_id: String },
    /// 逐字增量文本。
    Delta { session_id: String, text: String },
    /// agent 触发了某个工具（如 Bash / Edit）。
    ToolUse { session_id: String, name: String },
    /// 会话上下文被压缩（`system/compact_boundary`）。前端在消息流对应位置内嵌提示。
    Compaction { session_id: String },
    /// 运行完成的最终结果。
    Completed {
        session_id: String,
        text: String,
        cost_usd: Option<f64>,
        duration_ms: Option<u64>,
    },
    /// 用户取消。
    Cancelled { session_id: String },
    /// 运行出错（超时、进程异常、解析失败等）。
    Error { session_id: String, message: String },
}

/// 解析一行 stream-json。无关行返回 `None`（防御式：解析失败也返回 `None`，不 panic）。
pub fn parse_stream_json_line(session_id: &str, line: &str) -> Option<CodingAgentEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    match v.get("type")?.as_str()? {
        "stream_event" => {
            let event = v.get("event")?;
            if event.get("type")?.as_str()? != "content_block_delta" {
                return None;
            }
            let delta = event.get("delta")?;
            if delta.get("type")?.as_str()? != "text_delta" {
                return None;
            }
            let text = delta.get("text")?.as_str()?.to_string();
            Some(CodingAgentEvent::Delta {
                session_id: session_id.to_string(),
                text,
            })
        }
        "assistant" => {
            let content = v.get("message")?.get("content")?.as_array()?;
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                        return Some(CodingAgentEvent::ToolUse {
                            session_id: session_id.to_string(),
                            name: name.to_string(),
                        });
                    }
                }
            }
            None
        }
        "system" => {
            // 上下文压缩：headless claude 压缩会话历史时发一行 system/compact_boundary。
            // 其余 system 行（init 等）继续忽略。
            if v.get("subtype")?.as_str()? != "compact_boundary" {
                return None;
            }
            Some(CodingAgentEvent::Compaction {
                session_id: session_id.to_string(),
            })
        }
        "result" => {
            let is_error = v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false);
            let text = v
                .get("result")
                .and_then(|r| r.as_str())
                .unwrap_or_default()
                .to_string();
            if is_error {
                Some(CodingAgentEvent::Error {
                    session_id: session_id.to_string(),
                    message: if text.is_empty() {
                        "agent 返回错误".to_string()
                    } else {
                        text
                    },
                })
            } else {
                Some(CodingAgentEvent::Completed {
                    session_id: session_id.to_string(),
                    text,
                    cost_usd: v.get("total_cost_usd").and_then(|c| c.as_f64()),
                    duration_ms: v.get("duration_ms").and_then(|d| d.as_u64()),
                })
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"你好"}}}"#;
        assert_eq!(
            parse_stream_json_line("s1", line),
            Some(CodingAgentEvent::Delta {
                session_id: "s1".into(),
                text: "你好".into()
            })
        );
    }

    #[test]
    fn ignores_non_text_deltas() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{"}}}"#;
        assert_eq!(parse_stream_json_line("s1", line), None);
    }

    #[test]
    fn parses_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{}}]}}"#;
        assert_eq!(
            parse_stream_json_line("s1", line),
            Some(CodingAgentEvent::ToolUse {
                session_id: "s1".into(),
                name: "Bash".into()
            })
        );
    }

    #[test]
    fn parses_successful_result_with_cost() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"done","total_cost_usd":0.0123,"duration_ms":1500,"session_id":"abc"}"#;
        assert_eq!(
            parse_stream_json_line("s1", line),
            Some(CodingAgentEvent::Completed {
                session_id: "s1".into(),
                text: "done".into(),
                cost_usd: Some(0.0123),
                duration_ms: Some(1500),
            })
        );
    }

    #[test]
    fn parses_error_result() {
        let line = r#"{"type":"result","is_error":true,"result":"boom"}"#;
        assert_eq!(
            parse_stream_json_line("s1", line),
            Some(CodingAgentEvent::Error {
                session_id: "s1".into(),
                message: "boom".into()
            })
        );
    }

    #[test]
    fn parses_compact_boundary() {
        let line = r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"auto","pre_tokens":50000}}"#;
        assert_eq!(
            parse_stream_json_line("s1", line),
            Some(CodingAgentEvent::Compaction {
                session_id: "s1".into()
            })
        );
    }

    #[test]
    fn ignores_system_init_and_garbage() {
        assert_eq!(
            parse_stream_json_line("s1", r#"{"type":"system","subtype":"init"}"#),
            None
        );
        assert_eq!(parse_stream_json_line("s1", "not json"), None);
        assert_eq!(parse_stream_json_line("s1", ""), None);
    }
}
