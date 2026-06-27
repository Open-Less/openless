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

mod asr {
    pub mod local {
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
#[path = "../../src/recorder.rs"]
mod recorder;
#[path = "../../src/shortcut_binding.rs"]
mod shortcut_binding;
#[path = "../../src/types.rs"]
mod types;
#[cfg(target_os = "windows")]
#[path = "../../src/unicode_keystroke.rs"]
mod unicode_keystroke;
