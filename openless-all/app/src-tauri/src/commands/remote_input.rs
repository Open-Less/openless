//! 远程输入（局域网手机录音）命令面。
//!
//! 手机在同一局域网用浏览器打开 `https://<PC-IP>:<port>` 的 H5 录音页，经
//! WSS 把 16kHz PCM 推回 PC，由 Coordinator 当作"手机麦克风"喂进现有听写
//! 管线。本模块只暴露设置页需要的状态查询 / PIN 重置 / 语言同步命令；
//! 服务启停由 set_settings 里的 prefs diff 触发（见 settings.rs）。

use super::*;

#[tauri::command]
pub fn get_remote_input_status(
    coord: CoordinatorState<'_>,
) -> crate::remote_server::RemoteInputStatus {
    coord.remote_input_status()
}

#[tauri::command]
pub async fn list_local_ips(app: AppHandle) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::remote_server::discover_lan_addresses(&app)
            .ips
            .iter()
            .map(|ip| ip.to_string())
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn regenerate_remote_pin(coord: CoordinatorState<'_>) -> Result<String, String> {
    coord.regenerate_remote_pin()
}

/// 同步 PC 端界面语言到远程输入服务，H5 录音页据此显示对应语言。
///
/// 不要在这里 `run_on_main_thread` 刷托盘：本命令是同步 IPC，设置页一挂载就调用。
/// 等主线程会和 WebView 互相卡住，窗口变成「未响应」。
#[tauri::command]
pub fn set_remote_locale(coord: CoordinatorState<'_>, locale: String) {
    coord.set_remote_locale(locale);
}
