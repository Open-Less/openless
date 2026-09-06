use super::*;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDeviceStartResponse {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

impl From<openless_core::OAuthDeviceFlow> for GithubDeviceStartResponse {
    fn from(flow: openless_core::OAuthDeviceFlow) -> Self {
        Self {
            flow_id: flow.flow_id,
            user_code: flow.user_code,
            verification_uri: flow.verification_uri,
            interval: flow.interval_secs,
            expires_in: flow.expires_in_secs,
        }
    }
}

fn command_error(error: openless_core::BackendError) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn github_device_flow_start(
    core: CoreState<'_>,
) -> Result<GithubDeviceStartResponse, String> {
    core
        .services()
        .marketplace
        .start_device_flow()
        .await
        .map(GithubDeviceStartResponse::from)
        .map_err(command_error)
}

#[tauri::command]
pub async fn github_device_flow_poll(
    core: CoreState<'_>,
    flow_id: String,
) -> Result<openless_core::OAuthPollResult, String> {
    core
        .services()
        .marketplace
        .poll_device_flow(flow_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn github_device_flow_cancel(
    core: CoreState<'_>,
    flow_id: Option<String>,
) -> Result<(), String> {
    core
        .services()
        .marketplace
        .cancel_device_flow(flow_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_auth_status(
    core: CoreState<'_>,
) -> Result<openless_core::MarketplaceAuthStatus, String> {
    core
        .services()
        .marketplace
        .auth_status()
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_logout(core: CoreState<'_>) -> Result<(), String> {
    core
        .services()
        .marketplace
        .logout()
        .await
        .map_err(command_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_start_wire_preserves_legacy_field_names_without_the_device_secret() {
        let wire = GithubDeviceStartResponse::from(openless_core::OAuthDeviceFlow {
            flow_id: "opaque-flow".into(),
            user_code: "ABCD-EFGH".into(),
            verification_uri: "https://github.com/login/device".into(),
            expires_in_secs: 600,
            interval_secs: 7,
        });
        let value = serde_json::to_value(wire).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "flowId": "opaque-flow",
                "userCode": "ABCD-EFGH",
                "verificationUri": "https://github.com/login/device",
                "interval": 7,
                "expiresIn": 600,
            })
        );
        let serialized = value.to_string();
        assert!(!serialized.contains("deviceCode"));
        assert!(!serialized.contains("raw-device-secret"));
    }

    #[test]
    fn oauth_poll_wire_preserves_the_legacy_tagged_union() {
        assert_eq!(
            serde_json::to_value(openless_core::OAuthPollResult::SlowDown).unwrap(),
            serde_json::json!({"kind": "slowDown"})
        );
        assert_eq!(
            serde_json::to_value(openless_core::OAuthPollResult::Authorized {
                login: "octocat".into()
            })
            .unwrap(),
            serde_json::json!({"kind": "authorized", "login": "octocat"})
        );
    }
}
