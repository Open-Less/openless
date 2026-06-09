//! Android overlay window permission and foreground service integration.

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

pub fn show_android_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android_native_bridge::show_overlay();
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android overlay is only available on Android".to_string())
    }
}

pub fn hide_android_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android_native_bridge::hide_overlay();
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android overlay is only available on Android".to_string())
    }
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{AndroidOverlayPermissionResult, AndroidOverlayStatus};
    use crate::types::{AndroidOverlayPermissionState, AndroidOverlayStatus as Status};

    pub fn get_android_overlay_status() -> AndroidOverlayStatus {
        let granted = crate::android_jni::android::with_android_env(|mut ctx| {
            crate::android_jni::android::can_draw_overlays(&mut ctx.env, &ctx.context)
        })
        .unwrap_or(false);
        Status {
            permission: if granted {
                AndroidOverlayPermissionState::Granted
            } else {
                AndroidOverlayPermissionState::NotGranted
            },
            overlay_visible: crate::android_native_bridge::is_overlay_visible(),
            message: if granted {
                "悬浮窗权限已授予".to_string()
            } else {
                "请在系统设置中授予悬浮窗权限".to_string()
            },
        }
    }

    pub fn request_android_overlay_permission() -> AndroidOverlayPermissionResult {
        match crate::android_jni::android::with_android_env(|mut ctx| {
            crate::android_jni::android::start_activity_class(
                &mut ctx.env,
                &ctx.context,
                "com.openless.app.OverlayPermissionActivity",
            )
        }) {
            Ok(()) => AndroidOverlayPermissionResult {
                launched: true,
                message: "已打开悬浮窗权限设置".to_string(),
            },
            Err(error) => AndroidOverlayPermissionResult {
                launched: false,
                message: error,
            },
        }
    }
}
