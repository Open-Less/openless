//! 远程输入（局域网手机录音）命令面。
//!
//! 手机在同一局域网用浏览器打开 `https://<PC-IP>:<port>` 的 H5 录音页，经
//! WSS 把 16kHz PCM 推回 PC，由共享 Core 当作"手机麦克风"喂进现有听写
//! 管线。本模块只暴露设置页需要的状态查询 / PIN 重置 / 语言同步命令；
//! 服务启停由 set_settings 里的 prefs diff 触发（见 settings.rs）。

use super::*;

#[tauri::command]
pub async fn get_remote_input_status(
    core: CoreState<'_>,
) -> Result<crate::remote_server::RemoteInputStatus, String> {
    let status = core
        .services()
        .remote_input
        .status()
        .map_err(|error| error.message)?;
    let pin = core
        .services()
        .remote_input
        .read_pairing_pin()
        .await
        .map_err(|error| error.message)?;
    Ok(map_remote_input_status(status, pin))
}

fn map_remote_input_status(
    status: openless_core::RemoteInputStatus,
    pin: openless_core::SecretValue,
) -> crate::remote_server::RemoteInputStatus {
    crate::remote_server::RemoteInputStatus {
        running: status.running,
        starting: status.starting,
        port: status.port,
        pin: pin.into_exposed(),
        urls: status.urls,
        urls_stale: status.urls_stale,
    }
}

#[tauri::command]
pub async fn list_local_ips(core: CoreState<'_>) -> Result<Vec<String>, String> {
    core.services()
        .remote_input
        .list_local_ips()
        .await
        .map_err(|error| error.message)
}

#[tauri::command]
pub async fn regenerate_remote_pin(core: CoreState<'_>) -> Result<String, String> {
    core.services()
        .remote_input
        .regenerate_pairing_pin()
        .await
        .map_err(|error| error.message)?;
    core.services()
        .remote_input
        .read_pairing_pin()
        .await
        .map(openless_core::SecretValue::into_exposed)
        .map_err(|error| error.message)
}

/// 同步 PC 端界面语言到远程输入服务，H5 录音页据此显示对应语言。
#[tauri::command]
pub async fn set_remote_locale(
    app: AppHandle,
    core: CoreState<'_>,
    locale: String,
) -> Result<(), String> {
    core.services()
        .remote_input
        .set_locale(locale)
        .await
        .map_err(|error| error.message)?;
    let refresh_app = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        if let Err(err) = crate::refresh_tray_microphone_menu(&refresh_app) {
            log::warn!("[tray] refresh menu after locale change failed: {err}");
        }
    }) {
        log::warn!("[tray] dispatch locale refresh failed: {err}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_remote_status_command_preserves_the_legacy_secret_wire_shape() {
        let wire = map_remote_input_status(
            openless_core::RemoteInputStatus {
                enabled: true,
                running: true,
                starting: false,
                port: 9443,
                urls: vec!["https://192.168.1.2:9443".into()],
                urls_stale: false,
                locale: "zh-CN".into(),
                connection_count: 2,
                active_session_id: Some(openless_core::SessionId::new()),
            },
            openless_core::SecretValue::new("123456"),
        );

        assert_eq!(
            serde_json::to_value(wire).unwrap(),
            serde_json::json!({
                "running": true,
                "starting": false,
                "port": 9443,
                "pin": "123456",
                "urls": ["https://192.168.1.2:9443"],
                "urlsStale": false
            })
        );
    }
}
