//! Coding Agent 的 Tauri compatibility commands。
//!
//! 主窗口授权和旧 JSON wire 形状属于 Tauri host；provider 规则、参数校验、运行状态与取消
//! 统一通过 `openless-core::CodingAgentApi`，避免命令层保留第二份业务实现。

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{State, Window};

use openless_core::{CodingAgentPermissionMode, McpServerStatus};

fn ensure_main_window(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("coding agent commands are only allowed from the main window".to_string())
    }
}

fn command_error(error: openless_core::BackendError) -> String {
    error.message
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDetectionWire {
    pub installed: bool,
    pub version: Option<String>,
    pub exe: String,
    pub mcp_servers: Vec<McpServerStatus>,
    pub has_computer_use: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeDetectionWire {
    pub installed: bool,
    pub version: Option<String>,
    pub exe: String,
}

async fn detect(
    backend: &openless_core::OpenLessBackend,
    provider: openless_core::CodingAgentProvider,
    executable: Option<String>,
) -> Result<openless_core::CodingAgentAvailability, String> {
    backend
        .services()
        .coding_agent
        .detect(openless_core::CodingAgentDetectRequest {
            provider,
            executable,
        })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn coding_agent_detect(
    window: Window,
    backend: State<'_, Arc<openless_core::OpenLessBackend>>,
    exe: Option<String>,
) -> Result<ClaudeDetectionWire, String> {
    ensure_main_window(&window)?;
    let result = detect(
        backend.inner(),
        openless_core::CodingAgentProvider::ClaudeCodeCli,
        exe,
    )
    .await?;
    Ok(ClaudeDetectionWire {
        installed: result.installed,
        version: result.version,
        exe: result.executable,
        mcp_servers: result.mcp_servers,
        has_computer_use: result.has_computer_use,
    })
}

#[tauri::command]
pub async fn coding_agent_detect_cli(
    window: Window,
    backend: State<'_, Arc<openless_core::OpenLessBackend>>,
    provider: String,
    exe: Option<String>,
) -> Result<OpenCodeDetectionWire, String> {
    ensure_main_window(&window)?;
    let parsed = openless_core::CodingAgentProvider::from_pref(&provider);
    if !matches!(
        parsed,
        openless_core::CodingAgentProvider::CodexCli | openless_core::CodingAgentProvider::DshCli
    ) {
        return Err(format!("该后端不走通用检测: {provider}"));
    }
    let result = detect(backend.inner(), parsed, exe).await?;
    Ok(OpenCodeDetectionWire {
        installed: result.installed,
        version: result.version,
        exe: result.executable,
    })
}

#[tauri::command]
pub async fn coding_agent_detect_opencode(
    window: Window,
    backend: State<'_, Arc<openless_core::OpenLessBackend>>,
    exe: Option<String>,
) -> Result<OpenCodeDetectionWire, String> {
    ensure_main_window(&window)?;
    let result = detect(
        backend.inner(),
        openless_core::CodingAgentProvider::OpenCodeCli,
        exe,
    )
    .await?;
    Ok(OpenCodeDetectionWire {
        installed: result.installed,
        version: result.version,
        exe: result.executable,
    })
}

#[tauri::command]
pub async fn coding_agent_list_opencode_models(
    window: Window,
    backend: State<'_, Arc<openless_core::OpenLessBackend>>,
    exe: Option<String>,
    refresh: Option<bool>,
) -> Result<Vec<String>, String> {
    ensure_main_window(&window)?;
    backend
        .services()
        .coding_agent
        .list_models(openless_core::CodingAgentModelsRequest {
            provider: openless_core::CodingAgentProvider::OpenCodeCli,
            executable: exe,
            refresh: refresh.unwrap_or(true),
        })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn coding_agent_run_test(
    window: Window,
    backend: State<'_, Arc<openless_core::OpenLessBackend>>,
    prompt: String,
    exe: Option<String>,
    permission_mode: Option<CodingAgentPermissionMode>,
    workdir: Option<String>,
    model: Option<String>,
    max_budget_usd: Option<f64>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    backend
        .services()
        .coding_agent
        .run_test(openless_core::CodingAgentTestRequest {
            provider: openless_core::CodingAgentProvider::ClaudeCodeCli,
            executable: exe,
            prompt,
            permission_mode: permission_mode.unwrap_or_default(),
            workdir: workdir
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            model,
            max_budget_usd: max_budget_usd.or(Some(0.5)),
            timeout_secs: 120,
        })
        .await
        .map(|_| ())
        .map_err(command_error)
}

#[tauri::command]
pub async fn coding_agent_cancel_test(
    backend: State<'_, Arc<openless_core::OpenLessBackend>>,
) -> Result<(), String> {
    backend
        .services()
        .coding_agent
        .cancel_test()
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn coding_agent_command_risk(
    backend: State<'_, Arc<openless_core::OpenLessBackend>>,
    command: String,
) -> Result<Option<String>, String> {
    backend
        .services()
        .coding_agent
        .command_risk(command)
        .await
        .map(|assessment| assessment.reason)
        .map_err(command_error)
}
