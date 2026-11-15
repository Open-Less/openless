//! 后端事件 → 前端事件名。唯一的映射表：HTTP/SSE 宿主用它推送，
//! 任何新的渲染宿主也必须走这里，否则两条路径会慢慢漂。
use openless_core::{BackendEvent, BackendEventKind};

/// 后端事件 → 前端事件名 + payload。
///
/// 返回 `None` 表示这个事件前端不关心。
pub fn map_event(event: &BackendEvent) -> Option<(&'static str, serde_json::Value)> {
    Some(match &event.kind {
        BackendEventKind::DictationStateChanged(state) => (
            "dictation:state",
            serde_json::to_value(state).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::HistoryChanged(change) => (
            "history:changed",
            serde_json::to_value(change).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::CredentialsChanged(status) => (
            "credentials:changed",
            serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::PreferencesChanged(change) => (
            "settings:changed",
            serde_json::to_value(change).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::TranscriptDelta(delta) => (
            "dictation:transcript",
            serde_json::to_value(delta).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::QaLevel(level) => (
            "qa:level",
            serde_json::to_value(level).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::QaState(state) => (
            "qa:state",
            serde_json::to_value(state).unwrap_or(serde_json::Value::Null),
        ),
        // 云同步（E2EE）：这三个事件前端 CloudSyncSection 与
        // encryptedSyncUiBridge 都在订阅，之前没映射 → 云同步界面永不刷新。
        // 选区助手：润色开始/完成/失败都走这里，宿主可以据此显示一行提示。
        BackendEventKind::SelectionStateChanged(state) => (
            "selection:state",
            serde_json::to_value(state).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::CloudSyncStateChanged(event) => (
            "cloud-sync-e2ee:state",
            serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::CloudSyncConflictDetected(event) => (
            "cloud-sync-e2ee:conflict",
            serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        ),
        BackendEventKind::CloudSyncRestoreCompleted(event) => (
            "cloud-sync-e2ee:restored",
            serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        ),
        _ => return None,
    })
}
