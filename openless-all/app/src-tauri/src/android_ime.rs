//! Android IME integration — status queries and text commit path.
//!
//! JNI wiring lands after `tauri android init`; until then these functions return
//! honest stub states so the frontend can gate cross-app input UI.

use serde::Serialize;

use crate::types::AndroidImeStatus;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidImeCommitResult {
    pub committed: bool,
    pub message: String,
}

pub fn get_android_ime_status() -> AndroidImeStatus {
    #[cfg(target_os = "android")]
    {
        android_impl::get_android_ime_status()
    }

    #[cfg(not(target_os = "android"))]
    {
        use crate::types::AndroidImeState;

        AndroidImeStatus {
            state: AndroidImeState::NotAndroid,
            enabled: false,
            selected: false,
            message: "Android IME backend is only available on Android".to_string(),
        }
    }
}

/// Commit recognized text into the active input connection via OpenLessImeService.
pub fn commit_text(text: &str) -> AndroidImeCommitResult {
    #[cfg(target_os = "android")]
    {
        return android_impl::commit_text(text);
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = text;
        AndroidImeCommitResult {
            committed: false,
            message: "Android IME commit is only available on Android".to_string(),
        }
    }
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{AndroidImeCommitResult, AndroidImeStatus};
    use crate::types::{AndroidImeState, AndroidImeStatus as Status};

    pub fn get_android_ime_status() -> AndroidImeStatus {
        // TODO: JNI → check InputMethodManager if OpenLessImeService is enabled/selected.
        Status {
            state: AndroidImeState::NotEnabled,
            enabled: false,
            selected: false,
            message: "OpenLess 输入法尚未启用（Kotlin/JNI 接线后更新状态）".to_string(),
        }
    }

    pub fn commit_text(text: &str) -> AndroidImeCommitResult {
        if text.trim().is_empty() {
            return AndroidImeCommitResult {
                committed: false,
                message: "empty text".to_string(),
            };
        }
        // TODO: JNI → OpenLessImeService.commitText(text)
        log::info!(
            "[android-ime] commit stub (chars={}): JNI not wired yet",
            text.chars().count()
        );
        AndroidImeCommitResult {
            committed: false,
            message: "IME service not connected — enable OpenLess keyboard in system settings"
                .to_string(),
        }
    }
}
