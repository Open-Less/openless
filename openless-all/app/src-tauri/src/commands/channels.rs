//! IPC surface for channel card management.
//!
//! One card = one nameable, reorderable, toggleable provider configuration. A
//! single vendor can have multiple cards (multiple keys); the channel id and
//! `providerType` are then separate — the former is the map key, the latter
//! decides protocol routing. See the `ChannelMeta` docs in
//! `persistence::credentials`.
//!
//! Credentials themselves do not flow through here: the frontend calls
//! `read_credential` / `set_credential` per channel id (passing the channel id
//! as the `provider` argument), keeping secrets out of bulk list responses.

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

/// Reclaims a draft card left completely blank when the "add channel" dialog
/// closes; returns whether it was actually deleted.
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

/// Records one "test connection" result for the card to show latency or mark
/// failure.
///
/// The timestamp is taken in the backend, never trusted from the frontend — a
/// skewed frontend clock would render "3 minutes ago" as negative.
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
