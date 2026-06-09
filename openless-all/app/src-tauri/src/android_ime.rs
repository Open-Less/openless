//! Android IME integration — status queries and text commit path.

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

pub fn request_android_ime_settings() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android_jni::android::with_android_env(|mut ctx| {
            crate::android_jni::android::launch_input_method_settings(&mut ctx.env, &ctx.context)
        })
    }

    #[cfg(not(target_os = "android"))]
    {
        Err("Android IME settings are only available on Android".to_string())
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
        match crate::android_jni::android::with_android_env(|mut ctx| {
            crate::android_jni::android::ime_status(&mut ctx.env, &ctx.context)
        }) {
            Ok((enabled, selected)) => {
                let state = if selected {
                    AndroidImeState::Enabled
                } else if enabled {
                    AndroidImeState::NotEnabled
                } else {
                    AndroidImeState::NotEnabled
                };
                Status {
                    state,
                    enabled,
                    selected,
                    message: if selected {
                        "OpenLess 输入法已选中".to_string()
                    } else if enabled {
                        "OpenLess 输入法已启用，但未选中".to_string()
                    } else {
                        "请在系统设置中启用 OpenLess 输入法".to_string()
                    },
                }
            }
            Err(error) => Status {
                state: AndroidImeState::NotEnabled,
                enabled: false,
                selected: false,
                message: error,
            },
        }
    }

    pub fn commit_text(text: &str) -> AndroidImeCommitResult {
        if text.trim().is_empty() {
            return AndroidImeCommitResult {
                committed: false,
                message: "empty text".to_string(),
            };
        }
        match crate::android_jni::android::with_android_env(|mut ctx| {
            crate::android_jni::android::ime_commit_text(&mut ctx.env, text)
        }) {
            Ok(true) => AndroidImeCommitResult {
                committed: true,
                message: "committed via OpenLess IME".to_string(),
            },
            Ok(false) => AndroidImeCommitResult {
                committed: false,
                message: "IME service not connected — enable/select OpenLess keyboard".to_string(),
            },
            Err(error) => AndroidImeCommitResult {
                committed: false,
                message: error,
            },
        }
    }
}
