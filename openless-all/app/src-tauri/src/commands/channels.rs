//! 渠道卡片管理的 IPC 面。
//!
//! 一张卡片 = 一份可命名、可排序、可开关的供应商配置。同一家厂商可以有多张卡片
//! （多把 key），此时渠道 id 与 `providerType` 分离 —— 前者是 map key，后者决定
//! 协议路由。详见 `persistence::credentials` 里 `ChannelMeta` 的说明。
//!
//! 凭据本身不走这里：前端按渠道 id 调 `read_credential` / `set_credential`
//! （`provider` 参数传渠道 id），避免密钥随列表批量出栈。

use super::*;
use crate::persistence::{ChannelKind, ChannelSummary};

fn parse_kind(kind: &str) -> Result<ChannelKind, String> {
    ChannelKind::parse(kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_channels(window: Window, kind: String) -> Result<Vec<ChannelSummary>, String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || CredentialsVault::list_channels(kind))
        .await
        .map_err(|e| format!("channel list worker failed: {e}"))
}

#[tauri::command]
pub async fn create_channel(
    window: Window,
    kind: String,
    provider_type: String,
    name: String,
) -> Result<String, String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::create_channel(kind, &provider_type, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel create worker failed: {e}"))?
}

#[tauri::command]
pub async fn set_channel_provider_type(
    window: Window,
    kind: String,
    id: String,
    provider_type: String,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::set_channel_provider_type(kind, &id, &provider_type)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel provider type worker failed: {e}"))?
}

/// 关闭「添加渠道」弹窗时回收没填任何内容的草稿卡片；返回是否真的删了。
#[tauri::command]
pub async fn delete_channel_if_blank(
    window: Window,
    kind: String,
    id: String,
) -> Result<bool, String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::delete_channel_if_blank(kind, &id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel cleanup worker failed: {e}"))?
}

#[tauri::command]
pub async fn rename_channel(
    window: Window,
    kind: String,
    id: String,
    name: String,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::rename_channel(kind, &id, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel rename worker failed: {e}"))?
}

#[tauri::command]
pub async fn delete_channel(window: Window, kind: String, id: String) -> Result<(), String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::delete_channel(kind, &id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel delete worker failed: {e}"))?
}

#[tauri::command]
pub async fn set_channel_enabled(
    window: Window,
    kind: String,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::set_channel_enabled(kind, &id, enabled).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel toggle worker failed: {e}"))?
}

#[tauri::command]
pub async fn reorder_channels(
    window: Window,
    kind: String,
    ids: Vec<String>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::reorder_channels(kind, &ids).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel reorder worker failed: {e}"))?
}

/// 记录一次「测试连通」的结果，供卡片显示延迟或标红。
///
/// 时间戳在后端取，不信任前端传入 —— 前端时钟错乱会让"3 分钟前"显示成负数。
#[tauri::command]
pub async fn record_channel_test(
    window: Window,
    kind: String,
    id: String,
    ok: bool,
    latency_ms: Option<u32>,
    error: Option<String>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let kind = parse_kind(&kind)?;
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    tauri::async_runtime::spawn_blocking(move || {
        CredentialsVault::record_channel_test(kind, &id, ok, latency_ms, at, error)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("channel test record worker failed: {e}"))?
}
