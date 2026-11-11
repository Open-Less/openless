//! 后端事件 → Web 渲染层。
//!
//! 前端用 `listen()` / `listenX()` 订阅了 27 个事件；这里把它们统一映射成
//! `window.__openlessEvent(name, payloadJson)`，由页面里的桥转成 DOM 事件。
//!
//! 关键点：事件在 tokio 线程里收到，而调用 `evaluate_javascript` 必须在 GTK 主
//! 线程 —— 所以中间过一条 glib channel（`glib::MainContext::channel` 的等价物）。
use gtk::glib;
use webkit2gtk::WebViewExt;
use openless_core::{BackendEvent, BackendEventKind, OpenLessBackend};
use serde_json::json;
use std::sync::Arc;

/// 后端事件 → 前端事件名 + payload。
///
/// GTK 宿主走 `evaluate_javascript`，HTTP 宿主走 SSE，但**映射必须只有一份**，
/// 否则两条渲染路径会慢慢漂。返回 `None` 表示这个事件前端不关心。
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

/// 把事件名 + payload 推到页面。
fn forward(view: &webkit2gtk::WebView, name: &str, payload: serde_json::Value) {
    let js = format!(
        "window.__openlessEvent && window.__openlessEvent({}, {});",
        json!(name),
        json!(payload.to_string())
    );
    view.evaluate_javascript(&js, None::<&str>, None::<&str>, None::<&webkit2gtk::gio::Cancellable>, |_| {});
}

/// 订阅后端事件并转发。返回的线程随进程一起结束（后台常驻 GUI）。
pub fn spawn_backend_events(
    backend: Arc<OpenLessBackend>,
    runtime: Arc<tokio::runtime::Runtime>,
    view: webkit2gtk::WebView,
) {
    let (tx, rx) = glib::MainContext::channel::<(String, serde_json::Value)>(glib::Priority::DEFAULT);

    // 主线程：把事件交给页面。
    {
        let view = view.clone();
        rx.attach(None, move |(name, payload)| {
            forward(&view, &name, payload);
            glib::ControlFlow::Continue
        });
    }

    runtime.spawn(async move {
        let mut subscription = backend.subscribe();
        loop {
            let event: BackendEvent = match subscription.recv().await {
                Ok(event) => event,
                Err(_) => {
                    // 滞后：让前端重新拉一次快照（`refresh_*`）。
                    let _ = tx.send(("resync".into(), json!({})));
                    continue;
                }
            };
            let Some((name, payload)) = map_event(&event) else {
                continue;
            };
            let _ = tx.send((name.to_string(), payload));
        }
    });
}
