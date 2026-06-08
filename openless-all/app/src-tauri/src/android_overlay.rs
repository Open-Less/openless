//! Android overlay window permission and foreground service stubs.

use serde::Serialize;

use crate::types::AndroidOverlayStatus;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidOverlayPermissionResult {
    pub launched: bool,
    pub message: String,
}

pub fn get_android_overlay_status() -> AndroidOverlayStatus {
    #[cfg(target_os = "android")]
    {
        android_impl::get_android_overlay_status()
    }

    #[cfg(not(target_os = "android"))]
    {
        use crate::types::AndroidOverlayPermissionState;

        AndroidOverlayStatus {
            permission: AndroidOverlayPermissionState::NotAndroid,
            overlay_visible: false,
            message: "Android overlay is only available on Android".to_string(),
        }
    }
}

pub fn request_android_overlay_permission() -> AndroidOverlayPermissionResult {
    #[cfg(target_os = "android")]
    {
        android_impl::request_android_overlay_permission()
    }

    #[cfg(not(target_os = "android"))]
    {
        AndroidOverlayPermissionResult {
            launched: false,
            message: "Android overlay permission is only available on Android".to_string(),
        }
    }
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{AndroidOverlayPermissionResult, AndroidOverlayStatus};
    use crate::types::{AndroidOverlayPermissionState, AndroidOverlayStatus as Status};

    pub fn get_android_overlay_status() -> AndroidOverlayStatus {
        // TODO: JNI → Settings.canDrawOverlays(context)
        Status {
            permission: AndroidOverlayPermissionState::NotGranted,
            overlay_visible: false,
            message: "悬浮窗权限未授予（Kotlin/JNI 接线后更新状态）".to_string(),
        }
    }

    pub fn request_android_overlay_permission() -> AndroidOverlayPermissionResult {
        // TODO: JNI → start OverlayPermissionActivity
        log::info!("[android-overlay] permission request stub: JNI not wired yet");
        AndroidOverlayPermissionResult {
            launched: false,
            message: "Overlay permission activity not wired — copy android-scaffolding into gen/android"
                .to_string(),
        }
    }
}
