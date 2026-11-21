use super::*;

#[tauri::command]
pub async fn siwc_status(
    core: CoreState<'_>,
    window: Window,
    channel_id: String,
) -> Result<openless_core::siwc::SiwcStatus, String> {
    credentials::ensure_main_window(&window)?;
    let service = core
        .siwc_for_channel(&channel_id)
        .await
        .map_err(|e| e.to_string())?;
    let status = service
        .status(&channel_id)
        .await
        .map_err(|e| e.to_string())?;
    core.refresh_and_publish_credentials()
        .await
        .map_err(|e| e.to_string())?;
    Ok(status)
}
#[tauri::command]
pub async fn siwc_models(
    core: CoreState<'_>,
    window: Window,
    channel_id: String,
) -> Result<Vec<openless_core::siwc::SiwcModel>, String> {
    credentials::ensure_main_window(&window)?;
    core.siwc_for_channel(&channel_id)
        .await
        .map_err(|e| e.to_string())?
        .list_models(&channel_id)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn siwc_action(
    core: CoreState<'_>,
    window: Window,
    app: AppHandle,
    channel_id: String,
    action: String,
    registration_id: Option<String>,
) -> Result<serde_json::Value, String> {
    credentials::ensure_main_window(&window)?;
    let service = core
        .siwc_for_channel(&channel_id)
        .await
        .map_err(|e| e.to_string())?;
    if matches!(
        action.as_str(),
        "sign_in" | "add_account" | "enable_usage" | "select" | "disconnect"
    ) {
        core.invalidate_channel_test(openless_core::ChannelKind::Llm, channel_id.clone())
            .await
            .map_err(|e| e.to_string())?;
    }
    let result = match action.as_str() {
        "sign_in" | "add_account" | "enable_usage" => {
            let launch = core
                .begin_siwc_sign_in(
                    channel_id.clone(),
                    action == "add_account",
                    action == "enable_usage",
                )
                .await
                .map_err(|e| e.to_string())?;
            #[cfg(target_os = "android")]
            {
                if crate::android::siwc_authorization_lease(true).is_err() {
                    let _ = service.cancel_sign_in(&channel_id).await;
                    return Err(openless_core::siwc::error("browser_failed").to_string());
                }
                let service = service.clone();
                let channel = channel_id.clone();
                let attempt = launch.attempt_id.clone();
                tokio::spawn(async move {
                    while service.authorizing(&channel, &attempt) {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    let _ = crate::android::siwc_authorization_lease(false);
                });
            }
            #[cfg(not(target_os = "android"))]
            let opened = {
                use tauri_plugin_shell::ShellExt;
                app.shell()
                    .open(launch.authorization_url(), None)
                    .map_err(|_| ())
            };
            #[cfg(target_os = "android")]
            let opened = {
                let _ = app;
                crate::external_url::open_external_url(launch.authorization_url()).map_err(|_| ())
            };
            if opened.is_err() {
                let _ = service.cancel_sign_in(&channel_id).await;
                return Err(openless_core::siwc::error("browser_failed").to_string());
            }
            serde_json::json!({"attemptId": launch.attempt_id})
        }
        "cancel" => {
            service
                .cancel_sign_in(&channel_id)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::Value::Null
        }
        "select" => {
            service
                .select_account(
                    channel_id.clone(),
                    registration_id.ok_or("registrationId required")?,
                )
                .await
                .map_err(|e| e.to_string())?;
            serde_json::Value::Null
        }
        "disconnect" => serde_json::to_value(
            service
                .disconnect(&channel_id)
                .await
                .map_err(|e| e.to_string())?,
        )
        .map_err(|_| "siwc result invalid")?,
        "acknowledge" => {
            service
                .acknowledge_welcome(channel_id.clone())
                .await
                .map_err(|e| e.to_string())?;
            serde_json::Value::Null
        }
        "retry" => {
            service
                .retry_usage(channel_id.clone())
                .await
                .map_err(|e| e.to_string())?;
            serde_json::Value::Null
        }
        _ => return Err("unknown SIWC action".into()),
    };
    core.refresh_and_publish_credentials()
        .await
        .map_err(|e| e.to_string())?;
    Ok(result)
}
