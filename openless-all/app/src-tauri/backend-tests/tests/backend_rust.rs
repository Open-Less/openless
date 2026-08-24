//! Rust-only backend unit harness.
//!
//! 这个测试 crate 只把纯 Rust 后端模块按源码路径编进来，不链接完整 Tauri
//! `openless_lib`，避免 Windows CI 在 test harness 启动前被桌面运行时 DLL 拦截。
//! Cargo 以 `cfg(test)` 编译这些 path-included 模块，所以各模块自己的
//! `#[cfg(test)]` 单测会在这里实际执行（见 hotkey / recorder / insertion）。

#![allow(dead_code, unused_variables)]

#[cfg(target_os = "windows")]
extern crate self as tauri;

#[cfg(target_os = "windows")]
pub struct AppHandle<R: Runtime>(std::marker::PhantomData<R>);

#[cfg(target_os = "windows")]
pub trait Runtime {}

#[cfg(target_os = "linux")]
mod linux_fcitx {
    pub fn commit_text(_text: &str) -> Result<(), String> {
        Err("fcitx is unavailable in the Rust-only test harness".to_string())
    }

    pub fn sync_qa_binding(_trigger: Option<crate::types::HotkeyTrigger>) {}

    pub fn sync_selection_polish_binding(_trigger: Option<crate::types::HotkeyTrigger>) {}

    pub fn sync_translation_binding(_trigger: Option<crate::types::HotkeyTrigger>) {}
}

mod asr {
    pub mod local {
        pub const WHISPER_MODEL_ID: &str = "whisper-large-v3-turbo";

        #[derive(Clone, Copy)]
        pub enum ModelId {
            Small06b,
            Large17b,
            WhisperBase,
            WhisperSmall,
            WhisperMedium,
            WhisperLargeV3,
            WhisperLargeV3Turbo,
            WhisperLargeV3TurboQ5,
        }

        impl ModelId {
            pub fn from_str(value: &str) -> Option<Self> {
                match value {
                    "qwen3-asr-0.6b" => Some(Self::Small06b),
                    "qwen3-asr-1.7b" => Some(Self::Large17b),
                    "whisper-base" => Some(Self::WhisperBase),
                    "whisper-small" => Some(Self::WhisperSmall),
                    "whisper-medium" => Some(Self::WhisperMedium),
                    "whisper-large-v3" => Some(Self::WhisperLargeV3),
                    "whisper-large-v3-turbo" => Some(Self::WhisperLargeV3Turbo),
                    "whisper-large-v3-turbo-q5" => Some(Self::WhisperLargeV3TurboQ5),
                    _ => None,
                }
            }

            pub fn as_str(self) -> &'static str {
                match self {
                    Self::Small06b => "qwen3-asr-0.6b",
                    Self::Large17b => "qwen3-asr-1.7b",
                    Self::WhisperBase => "whisper-base",
                    Self::WhisperSmall => "whisper-small",
                    Self::WhisperMedium => "whisper-medium",
                    Self::WhisperLargeV3 => "whisper-large-v3",
                    Self::WhisperLargeV3Turbo => "whisper-large-v3-turbo",
                    Self::WhisperLargeV3TurboQ5 => "whisper-large-v3-turbo-q5",
                }
            }

            pub fn is_qwen(self) -> bool {
                matches!(self, Self::Small06b | Self::Large17b)
            }

            pub fn is_whisper(self) -> bool {
                !self.is_qwen()
            }
        }

        pub mod foundry {
            pub const DEFAULT_MODEL_ALIAS: &str = "whisper-large-v3-turbo";
            pub const PROVIDER_ID: &str = "foundry-local-whisper";
        }

        pub mod foundry_native {
            pub fn normalize_runtime_source_str(value: &str) -> String {
                match value.trim() {
                    "nuget" | "ort-nightly" => value.trim().to_string(),
                    _ => "auto".to_string(),
                }
            }
        }

        pub mod sherpa {
            pub const DEFAULT_MODEL_ALIAS: &str = "sense-voice-small-zh";
            pub const PROVIDER_ID: &str = "sherpa-onnx-local";

            pub fn is_sherpa_onnx_local(id: &str) -> bool {
                id == PROVIDER_ID
            }
        }
    }
}

#[path = "../../src/coordinator_state.rs"]
mod coordinator_state;
mod selection {
    pub fn prefetch_selection_workspace_capture() {}
}
#[path = "../../src/global_hotkey_runtime.rs"]
mod global_hotkey_runtime;
#[path = "../../src/combo_hotkey.rs"]
mod combo_hotkey;
#[path = "../../src/side_aware_combo.rs"]
mod side_aware_combo;
#[path = "../../src/hotkey.rs"]
mod hotkey;
#[cfg(not(target_os = "macos"))]
#[path = "../../src/insertion.rs"]
mod insertion;
#[path = "../../src/remote_server/pin_persistence.rs"]
mod pin_persistence;
#[path = "../../src/recorder.rs"]
mod recorder;
#[path = "../../src/shortcut_binding.rs"]
mod shortcut_binding;
#[path = "../../src/types.rs"]
mod types;
#[cfg(target_os = "windows")]
#[path = "../../src/unicode_keystroke.rs"]
mod unicode_keystroke;
#[path = "../../src/windows_ime_profile.rs"]
mod windows_ime_profile;
#[path = "../../src/windows_ime_restore.rs"]
mod windows_ime_restore;
