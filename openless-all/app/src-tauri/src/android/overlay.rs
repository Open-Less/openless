//! Android overlay window permission and foreground service integration.

use serde::Serialize;

use crate::android::types::AndroidOverlayStatus;

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
        use crate::android::types::AndroidOverlayPermissionState;

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
        return crate::android::native_bridge::show_overlay();
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android overlay is only available on Android".to_string())
    }
}

pub fn hide_android_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::native_bridge::hide_overlay();
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android overlay is only available on Android".to_string())
    }
}

pub fn refresh_android_overlay_if_visible() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::native_bridge::refresh_overlay_if_visible();
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android overlay is only available on Android".to_string())
    }
}

pub fn refresh_android_overlay_layout() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::native_bridge::refresh_overlay_layout();
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android overlay is only available on Android".to_string())
    }
}

pub fn replace_android_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::native_bridge::replace_overlay();
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android overlay is only available on Android".to_string())
    }
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{AndroidOverlayPermissionResult, AndroidOverlayStatus};
    use crate::android::types::{AndroidOverlayPermissionState, AndroidOverlayStatus as Status};

    pub fn get_android_overlay_status() -> AndroidOverlayStatus {
        let granted = crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::can_draw_overlays(env, context)
        })
        .unwrap_or(false);
        Status {
            permission: if granted {
                AndroidOverlayPermissionState::Granted
            } else {
                AndroidOverlayPermissionState::NotGranted
            },
            overlay_visible: crate::android::native_bridge::is_overlay_visible(),
            message: if granted {
                "悬浮窗权限已授予".to_string()
            } else {
                "请在系统设置中授予悬浮窗权限".to_string()
            },
        }
    }

    pub fn request_android_overlay_permission() -> AndroidOverlayPermissionResult {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::start_activity_class(
                env,
                context,
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
