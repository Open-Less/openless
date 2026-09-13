//! 渠道卡片管理的 IPC 面。
//!
//! 一张卡片 = 一份可命名、可排序、可开关的供应商配置。同一家厂商可以有多张卡片
//! （多把 key），此时渠道 id 与 `providerType` 分离 —— 前者是 map key，后者决定
//! 协议路由。详见 `persistence::credentials` 里 `ChannelMeta` 的说明。
//!
//! 凭据本身不走这里：前端按渠道 id 调 `read_credential` / `set_credential`
//! （`provider` 参数传渠道 id），避免密钥随列表批量出栈。

use super::*;
use openless_core::{ChannelKind, ChannelSummary};

fn parse_kind(kind: &str) -> Result<ChannelKind, String> {
    ChannelKind::parse(kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_channels(
    core: CoreState<'_>,
    window: Window,
    kind: String,
) -> Result<Vec<ChannelSummary>, String> {
    ensure_main_window(&window)?;
    core.list_channels(parse_kind(&kind)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_channel(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    provider_type: String,
    name: String,
) -> Result<String, String> {
    ensure_main_window(&window)?;
    core.create_channel(parse_kind(&kind)?, provider_type, name)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_channel_provider_type(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    id: String,
    provider_type: String,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    core.set_channel_provider_type(parse_kind(&kind)?, id, provider_type)
        .await
        .map_err(|error| error.to_string())
}

/// 关闭「添加渠道」弹窗时回收没填任何内容的草稿卡片；返回是否真的删了。
#[tauri::command]
pub async fn delete_channel_if_blank(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    id: String,
) -> Result<bool, String> {
    ensure_main_window(&window)?;
    core.delete_channel_if_blank(parse_kind(&kind)?, id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rename_channel(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    id: String,
    name: String,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    core.rename_channel(parse_kind(&kind)?, id, name)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_channel(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    id: String,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    core.delete_channel(parse_kind(&kind)?, id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_channel_enabled(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    core.set_channel_enabled(parse_kind(&kind)?, id, enabled)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn reorder_channels(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    ids: Vec<String>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    core.reorder_channels(parse_kind(&kind)?, ids)
        .await
        .map_err(|error| error.to_string())
}

/// 记录一次「测试连通」的结果，供卡片显示延迟或标红。
///
/// 时间戳在后端取，不信任前端传入 —— 前端时钟错乱会让"3 分钟前"显示成负数。
#[tauri::command]
pub async fn record_channel_test(
    core: CoreState<'_>,
    window: Window,
    kind: String,
    id: String,
    ok: bool,
    latency_ms: Option<u32>,
    error: Option<String>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    core.record_channel_test(parse_kind(&kind)?, id, ok, latency_ms, error)
        .await
        .map_err(|error| error.to_string())
}
