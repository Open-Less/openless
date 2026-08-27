//! QA 事件载荷 —— 对应后端 `qa:state` 事件（`coordinator/qa_session.rs` emit 的 JSON）。
//!
//! 顶层字段为 snake_case（`session_id` / `selection_preview` / `chunk`），`messages`
//! 数组元素 `QaChatMessage` 序列化为 camelCase（`selectionText`）。字段全部可缺省，
//! 便于用 `serde_json::from_value` 容错解析（后端不同 kind 只带部分字段）。
//!
//! 寄生在 `egui_host` 内避免污染 `types.rs` 的公开契约；`apply_qa_state` 依赖它。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct QaStateEvent {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub messages: Option<Vec<crate::types::QaChatMessage>>,
    #[serde(default)]
    pub selection_preview: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub chunk: Option<String>,
}
