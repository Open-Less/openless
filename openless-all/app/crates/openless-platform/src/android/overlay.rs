//! Android overlay window permission and foreground service integration.

use serde::Serialize;

use openless_core::android_types::AndroidOverlayStatus;

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

    
}

pub fn request_android_overlay_permission() -> AndroidOverlayPermissionResult {
    #[cfg(target_os = "android")]
    {
        android_impl::request_android_overlay_permission()
    }

    
}

pub fn show_android_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::overlay_control::show_overlay();
    }
    
}

pub fn hide_android_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::overlay_control::hide_overlay();
    }
    
}

pub fn refresh_android_overlay_if_visible() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::overlay_control::refresh_overlay_if_visible();
    }
    
}

pub fn refresh_android_overlay_layout() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::overlay_control::refresh_overlay_layout();
    }
    
}

pub fn replace_android_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return crate::android::overlay_control::replace_overlay();
    }
    
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{AndroidOverlayPermissionResult, AndroidOverlayStatus};
    use openless_core::android_types::{AndroidOverlayPermissionState, AndroidOverlayStatus as Status};

    pub fn get_android_overlay_status() -> AndroidOverlayStatus {
        let granted = match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::can_draw_overlays(env, context)
        }) {
            Ok(granted) => granted,
            Err(error) => {
                // #region agent log
                log::warn!(
                    "[OpenLessDbg58c22b] {{\"sessionId\":\"58c22b\",\"hypothesisId\":\"A\",\"location\":\"overlay::get_android_overlay_status\",\"message\":\"status query failed\",\"data\":{{\"error\":\"{error}\"}},\"timestamp\":{}}}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                );
                // #endregion
                log::warn!("[android-overlay] status query failed: {error}");
                false
            }
        };
        Status {
            permission: if granted {
                AndroidOverlayPermissionState::Granted
            } else {
                AndroidOverlayPermissionState::NotGranted
            },
            overlay_visible: crate::android::overlay_control::is_overlay_visible(),
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
