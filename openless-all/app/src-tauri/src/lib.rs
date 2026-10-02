//! OpenLess Tauri backend.
//!
//! Modules mirror the original Swift libraries (one purpose per file):
//! - hotkey: global hotkey monitor
//! - recorder: microphone capture (16 kHz mono Int16 PCM)
//! - asr: streaming ASR providers (Volcengine SAUC bigmodel)
//! - polish: OpenAI-compatible chat completions client
//! - insertion: cursor-position text insertion (AX / paste)
//! - persistence: history + preferences + credentials vault
//! - coordinator: dictation state machine glue
//! - commands: Tauri IPC surface

// ── Linux 桌面不再由 Tauri 提供 ───────────────────────────────────────────────
// 用户在 Linux 上已使用 egui 前端（`openless-all/app/linux-egui`），Tauri 版的 Linux
// 桌面实现（fcitx5 插件桥 `linux_fcitx.rs`、Linux 热键/插入/选区路径、平台覆盖配置
// `tauri.linux.conf.json`）已随本次一并删除。这里显式拦住误在 Linux 上构建 Tauri 版
// 的情况，给出可操作的提示，而不是让人面对一堆“找不到模块”的零散错误。
//
// 不受影响的平台：macOS / Windows（桌面）与 Android（`target_os = "android"`）。
#[cfg(target_os = "linux")]
compile_error!(
    "Tauri 版已不再支持 Linux 桌面：请使用 egui 前端 openless-all/app/linux-egui，\
     打包用 openless-all/app/scripts/package-linux-egui.sh（deb/rpm/manual zip）。"
);

mod android;
mod asr;
mod audio_mute;
#[cfg(test)]
mod build_target;
mod cli;
mod coding_agent;
#[cfg(not(mobile))]
mod combo_hotkey;
#[cfg(mobile)]
#[path = "mobile_stubs/combo_hotkey.rs"]
mod combo_hotkey;
mod commands;
mod coordinator;
mod coordinator_state;
mod core_adapters;
mod correction;
#[cfg(target_os = "macos")]
mod macos_dictation_key;
#[cfg(target_os = "macos")]
mod macos_streaming_input;
mod qa_adapter;
mod tauri_coordinator_host;
// 托盘麦克风设备变更监听：macOS CoreAudio / Windows MMDevice 原生通知（空闲零唤醒），
// 仅桌面端。详见 issue #470。
#[cfg(not(mobile))]
mod device_watch;
mod endpoint_security;
mod external_url;
#[cfg(not(mobile))]
mod global_hotkey_runtime;
// Reads the host app's text around the cursor as LLM polish context. The only place that
// touches other apps' documents; all platform differences and security guards live here.
// Currently macOS-only; other platforms degrade gracefully.
mod host_document;
#[cfg(not(mobile))]
#[path = "hotkey.rs"]
mod hotkey;
#[cfg(mobile)]
#[path = "mobile_stubs/hotkey.rs"]
mod hotkey;
mod insertion;
mod llm_gemini;
#[cfg(mobile)]
mod mobile_runtime;
#[cfg(not(mobile))]
mod mouse_dictation;
#[cfg(mobile)]
#[path = "mobile_stubs/mouse_dictation.rs"]
mod mouse_dictation;
mod net;
mod omni;
mod permissions;
mod persistence;
mod polish;
#[cfg(not(mobile))]
mod qa_hotkey;
#[cfg(mobile)]
#[path = "mobile_stubs/qa_hotkey.rs"]
mod qa_hotkey;
mod recorder;
#[cfg(not(mobile))]
mod remote_server;
#[cfg(not(mobile))]
#[path = "selection.rs"]
mod selection;
#[cfg(mobile)]
#[path = "mobile_stubs/selection.rs"]
mod selection;
#[cfg(not(mobile))]
mod shortcut_binding;
#[cfg(mobile)]
#[path = "mobile_stubs/shortcut_binding.rs"]
mod shortcut_binding;
#[cfg(not(mobile))]
mod side_aware_combo;
#[cfg(mobile)]
#[path = "mobile_stubs/side_aware_combo.rs"]
mod side_aware_combo;
mod tauri_events;
mod types;
#[cfg(not(mobile))]
mod unicode_keystroke;
#[cfg(mobile)]
#[path = "mobile_stubs/unicode_keystroke.rs"]
mod unicode_keystroke;
#[cfg(target_os = "windows")]
mod windows_ime_ipc;
mod windows_ime_profile;
#[cfg(target_os = "windows")]
mod windows_ime_protocol;
mod windows_ime_restore;
#[cfg(target_os = "windows")]
mod windows_ime_session;
#[cfg(target_os = "windows")]
mod windows_ime_target;

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

const LOG_ROTATE_LIMIT_BYTES: u64 = 10 * 1024 * 1024;
#[cfg(target_os = "macos")]
const OPENLESS_BUNDLE_ID: &str = "com.openless.app";

/// Positions the QA window at the bottom-center of the screen on first show only, so the
/// user-dragged position survives hide → show cycles. See issue #118 v2.
static QA_WINDOW_POSITIONED: AtomicBool = AtomicBool::new(false);
/// 「润色结果」模式的事件名。该模式复用选区助手（qa）面板，不再有独立预览窗。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) const SELECTION_POLISH_PREVIEW_SHOWN: &str = "selection-polish-preview:shown";
/// 让面板自行决定退出「润色结果」模式（它可能正处在提问对话中）。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) const SELECTION_POLISH_PREVIEW_HIDE: &str = "selection-polish-preview:hide";
/// 是否处于「润色结果」模式。`get_selection_polish_preview` 只在它为 true 时
/// 返回负载：面板懒创建后挂载时也会拉一次，不能把残留的快照当成新请求。
static SELECTION_POLISH_PREVIEW_PENDING: AtomicBool = AtomicBool::new(false);

pub(crate) fn selection_polish_preview_pending() -> bool {
    SELECTION_POLISH_PREVIEW_PENDING.load(Ordering::SeqCst)
}

pub(crate) fn clear_selection_polish_preview_pending() {
    SELECTION_POLISH_PREVIEW_PENDING.store(false, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
static LESS_COMPUTER_WINDOW_POSITIONED: AtomicBool = AtomicBool::new(false);

/// 聊天面板退场动画的世代计数：hide 先发 `chat-panel:closing` 让前端播 220ms
/// 退场动画、240ms 后才真正 hide；期间再次 show 会推进世代，作废挂起的 hide。
static QA_PANEL_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LESS_COMPUTER_PANEL_EPOCH: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(not(mobile))]
static TRAY_MICROPHONE_WATCHER_STOPPING: AtomicBool = AtomicBool::new(false);
#[cfg(not(mobile))]
struct TrayMicrophoneDeviceCache(parking_lot::Mutex<Vec<recorder::MicrophoneDevice>>);
#[cfg(not(mobile))]
use tauri::menu::{
    CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, Submenu, SubmenuBuilder,
};
#[cfg(not(mobile))]
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    RunEvent, Runtime,
};
// Desktop only: the mobile WebviewWindowBuilder lacks decorations/shadow methods, so lazy
// creation is desktop-only.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[cfg(not(mobile))]
use crate::types::{PolishMode, StylePack, StylePackKind};

#[cfg(test)]
pub(crate) fn set_backend_preferences_for_test(
    backend: &openless_core::OpenLessBackend,
    preferences: crate::types::UserPreferences,
) {
    backend
        .update_settings(
            preferences,
            openless_core::SettingsUpdateOptions::STRICT,
            &openless_core::NoopSettingsRuntime,
        )
        .expect("test preferences must satisfy the public settings contract");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    asr::local::run_mlx_worker_if_requested();

    #[cfg(mobile)]
    {
        mobile_runtime::run();
        return;
    }
    #[cfg(not(mobile))]
    run_desktop();
}

macro_rules! app_invoke_handler_desktop {
    () => {
        tauri::generate_handler![
            commands::get_startup_snapshot,
            commands::get_settings,
            commands::get_settings_snapshot,
            commands::update_setting_fields,
            commands::get_default_style_system_prompts,
            commands::set_settings,
            commands::get_remote_input_status,
            commands::list_local_ips,
            commands::regenerate_remote_pin,
            commands::set_remote_locale,
            commands::get_update_channel,
            commands::set_update_channel,
            commands::fetch_latest_beta_release,
            commands::app_check_update_with_channel,
            commands::check_network,
            commands::take_splash_playback,
            commands::get_hotkey_status,
            commands::get_hotkey_capability,
            commands::set_shortcut_recording_active,
            commands::get_windows_ime_status,
            commands::get_platform_capabilities,
            commands::get_android_overlay_status,
            commands::request_android_overlay_permission,
            commands::show_android_overlay,
            commands::hide_android_overlay,
            commands::get_android_accessibility_status,
            commands::request_android_accessibility_permission,
            commands::get_android_shizuku_status,
            commands::request_android_shizuku_permission,
            commands::open_shizuku_app,
            commands::recover_android_accessibility,
            commands::open_external_url,
            commands::list_microphone_devices,
            commands::start_microphone_level_monitor,
            commands::stop_microphone_level_monitor,
            commands::get_credentials,
            commands::set_credential,
            commands::list_history,
            commands::delete_history_entry,
            commands::clear_history,
            commands::get_activity_stats,
            commands::read_audio_recording,
            commands::export_audio_recording,
            commands::retranscribe_recording,
            commands::apply_quick_note_repolish,
            commands::marketplace_list,
            commands::marketplace_detail,
            commands::marketplace_install,
            commands::marketplace_download,
            commands::marketplace_upload,
            commands::marketplace_like,
            commands::marketplace_my_likes,
            commands::marketplace_my_packs,
            commands::marketplace_delete,
            commands::github_device_flow_start,
            commands::github_device_flow_poll,
            commands::github_device_flow_cancel,
            commands::marketplace_auth_status,
            commands::cloud_sync_status,
            commands::cloud_sync_upload,
            commands::cloud_sync_restore,
            commands::cloud_sync_delete,
            commands::cloud_sync_e2ee_status,
            commands::cloud_sync_e2ee_claim_setup_prompt,
            commands::cloud_sync_e2ee_prepare_enable,
            commands::cloud_sync_e2ee_create,
            commands::cloud_sync_e2ee_unlock,
            commands::cloud_sync_e2ee_lock,
            commands::cloud_sync_e2ee_set_enabled,
            commands::cloud_sync_e2ee_sync_now,
            commands::cloud_sync_e2ee_cancel,
            commands::cloud_sync_e2ee_preview_restore,
            commands::cloud_sync_e2ee_apply_restore,
            commands::cloud_sync_e2ee_change_password,
            commands::cloud_sync_e2ee_delete_remote,
            commands::cloud_sync_e2ee_sign_out,
            commands::cloud_sync_e2ee_sign_in_with_token,
            commands::cloud_sync_e2ee_begin_sign_in,
            commands::cloud_sync_e2ee_poll_sign_in,
            commands::cloud_sync_e2ee_cancel_sign_in,
            commands::cloud_sync_e2ee_get_ui_preferences,
            commands::cloud_sync_e2ee_get_ui_preferences_snapshot,
            commands::cloud_sync_e2ee_set_ui_preferences_checked,
            commands::marketplace_logout,
            commands::list_vocab,
            commands::add_vocab,
            commands::remove_vocab,
            commands::set_vocab_enabled,
            commands::update_vocab,
            commands::list_correction_rules,
            commands::add_correction_rule,
            commands::remove_correction_rule,
            commands::set_correction_rule_enabled,
            commands::list_vocab_presets,
            commands::save_vocab_presets,
            commands::start_dictation,
            commands::stop_dictation,
            commands::cancel_dictation,
            coding_agent::commands::coding_agent_detect,
            coding_agent::commands::coding_agent_detect_opencode,
            coding_agent::commands::coding_agent_detect_cli,
            coding_agent::commands::coding_agent_list_opencode_models,
            coding_agent::commands::coding_agent_run_test,
            coding_agent::commands::coding_agent_cancel_test,
            coding_agent::commands::coding_agent_command_risk,
            commands::handle_window_hotkey_event,
            #[cfg(debug_assertions)]
            commands::inject_hotkey_click_for_dev,
            #[cfg(debug_assertions)]
            commands::run_selection_polish_for_dev,
            #[cfg(not(mobile))]
            commands::get_selection_polish_preview,
            #[cfg(not(mobile))]
            commands::confirm_selection_polish_preview,
            #[cfg(not(mobile))]
            commands::cancel_selection_polish_preview,
            commands::repolish,
            commands::list_style_packs,
            commands::create_style_pack_from_template,
            commands::save_style_pack,
            commands::set_style_pack_icon,
            commands::read_style_pack_icon,
            commands::preview_style_pack_runtime,
            commands::set_active_style_pack,
            commands::set_style_pack_enabled,
            commands::reset_builtin_style_pack,
            commands::delete_style_pack,
            commands::import_style_pack_from_zip,
            commands::export_style_pack_to_zip,
            commands::set_default_polish_mode,
            commands::set_style_enabled,
            commands::check_accessibility_permission,
            commands::request_accessibility_permission,
            commands::check_microphone_permission,
            commands::request_microphone_permission,
            commands::open_system_settings,
            commands::trigger_microphone_prompt,
            commands::read_credential,
            commands::set_active_asr_provider,
            commands::set_active_llm_provider,
            commands::list_channels,
            commands::create_channel,
            commands::rename_channel,
            commands::set_channel_provider_type,
            commands::delete_channel_if_blank,
            commands::delete_channel,
            commands::set_channel_enabled,
            commands::reorder_channels,
            commands::record_channel_test,
            commands::set_active_omni_provider,
            commands::get_qa_hotkey_label,
            commands::set_qa_hotkey,
            commands::set_selection_polish_hotkey,
            #[cfg(all(not(mobile), target_os = "windows"))]
            commands::get_selection_voice_intent_prompt,
            #[cfg(all(not(mobile), target_os = "windows"))]
            commands::confirm_selection_voice_intent_prompt,
            #[cfg(all(not(mobile), target_os = "windows"))]
            commands::cancel_selection_voice_intent_prompt,
            #[cfg(all(not(mobile), target_os = "windows"))]
            commands::get_selection_voice_preview,
            #[cfg(all(not(mobile), target_os = "windows"))]
            commands::confirm_selection_voice_preview,
            #[cfg(all(not(mobile), target_os = "windows"))]
            #[cfg(all(not(mobile), target_os = "windows"))]
            commands::revert_selection_voice_preview,
            commands::validate_shortcut_binding,
            commands::set_dictation_hotkey,
            commands::set_translation_hotkey,
            commands::set_switch_style_hotkey,
            commands::set_open_app_hotkey,
            commands::set_quick_note_hotkey,
            commands::set_style_pack_hotkeys,
            commands::qa_window_dismiss,
            commands::qa_window_set_expanded,
            commands::qa_get_snapshot,
            commands::qa_toggle_recording,
            commands::qa_submit_text,
            commands::qa_set_edit_instruction_mode,
            commands::less_computer_window_dismiss,
            commands::less_computer_window_open,
            commands::chat_panel_focus_keyboard,
            commands::less_computer_submit_text,
            commands::less_computer_sync,
            commands::less_computer_approve,
            commands::less_computer_voice_start,
            commands::less_computer_voice_stop,
            commands::less_computer_voice_cancel,
            commands::less_computer_task_cancel,
            commands::validate_combo_hotkey,
            commands::set_combo_hotkey,
            commands::list_provider_descriptors,
            commands::validate_provider_credentials,
            commands::list_provider_models,
            commands::local_asr_get_settings,
            commands::local_asr_storage_settings,
            commands::local_asr_set_models_base_dir,
            commands::local_asr_activate,
            commands::local_asr_set_active_model,
            commands::local_asr_set_mirror,
            commands::local_asr_list_models,
            commands::local_asr_fetch_remote_info,
            commands::local_asr_fetch_hf_card,
            commands::local_asr_download_model,
            commands::local_asr_cancel_download,
            commands::local_asr_delete_model,
            commands::local_asr_cleanup_incomplete,
            commands::local_asr_model_dir,
            commands::local_asr_reveal_model_dir,
            commands::local_asr_reveal_models_root,
            commands::local_asr_test_model,
            commands::local_asr_test_channel,
            commands::local_asr_engine_status,
            commands::local_asr_release_engine,
            commands::local_asr_preload,
            commands::local_asr_set_keep_loaded_secs,
            commands::foundry_local_asr_status,
            commands::foundry_local_asr_catalog,
            commands::foundry_local_asr_set_model,
            commands::foundry_local_asr_set_language_hint,
            commands::foundry_local_asr_set_runtime_source,
            commands::foundry_local_asr_set_keep_loaded_secs,
            commands::foundry_local_asr_prepare,
            commands::foundry_local_asr_cancel_prepare,
            commands::foundry_local_asr_release,
            commands::foundry_local_asr_model_dir,
            commands::foundry_local_asr_delete_model,
            commands::foundry_local_asr_reveal_model_dir,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_status,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_catalog,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_fetch_remote_info,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_download_model,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_cancel_download,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_set_model,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_set_language_hint,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_prepare,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_cancel_prepare,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_release,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_model_dir,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_delete_model,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_reveal_model_dir,
            commands::export_error_log,
            commands::debug_read_cursor_context,
            commands::accept_pending_correction,
            commands::add_learned_vocab,
            commands::reject_pending_correction,
            commands::dismiss_vocab_suggestions,
            commands::copy_text_to_clipboard,
            commands::dismiss_insert_fallback_card,
            commands::report_insert_fallback_card_height,
            restart_app,
            reset_accessibility_permission_and_restart_app,
            log_client_error,
            set_windows_caption_theme,
        ]
    };
}

/// Android/iOS: only commands usable without desktop hotkeys, tray, updater, or local ASR.
#[macro_export]
macro_rules! app_invoke_handler_mobile {
    () => {
        tauri::generate_handler![
            $crate::commands::get_startup_snapshot,
            $crate::commands::get_settings,
            $crate::commands::get_settings_snapshot,
            $crate::commands::update_setting_fields,
            $crate::commands::get_default_style_system_prompts,
            $crate::commands::set_settings,
            $crate::commands::check_network,
            $crate::commands::take_splash_playback,
            $crate::commands::get_platform_capabilities,
            $crate::commands::get_android_overlay_status,
            $crate::commands::request_android_overlay_permission,
            $crate::commands::show_android_overlay,
            $crate::commands::hide_android_overlay,
            $crate::commands::get_android_accessibility_status,
            $crate::commands::request_android_accessibility_permission,
            $crate::commands::get_android_shizuku_status,
            $crate::commands::request_android_shizuku_permission,
            $crate::commands::open_shizuku_app,
            $crate::commands::recover_android_accessibility,
            $crate::commands::open_external_url,
            $crate::commands::list_microphone_devices,
            $crate::commands::start_microphone_level_monitor,
            $crate::commands::stop_microphone_level_monitor,
            $crate::commands::get_credentials,
            $crate::commands::set_credential,
            $crate::commands::read_credential,
            $crate::commands::set_active_asr_provider,
            $crate::commands::set_active_llm_provider,
            $crate::commands::list_channels,
            $crate::commands::create_channel,
            $crate::commands::rename_channel,
            $crate::commands::set_channel_provider_type,
            $crate::commands::delete_channel_if_blank,
            $crate::commands::delete_channel,
            $crate::commands::set_channel_enabled,
            $crate::commands::reorder_channels,
            $crate::commands::record_channel_test,
            $crate::commands::set_active_omni_provider,
            $crate::commands::list_provider_descriptors,
            $crate::commands::validate_provider_credentials,
            $crate::commands::list_provider_models,
            $crate::commands::list_history,
            $crate::commands::delete_history_entry,
            $crate::commands::clear_history,
            $crate::commands::get_activity_stats,
            $crate::commands::read_audio_recording,
            $crate::commands::export_audio_recording,
            $crate::commands::retranscribe_recording,
            $crate::commands::apply_quick_note_repolish,
            $crate::commands::marketplace_list,
            $crate::commands::marketplace_detail,
            $crate::commands::marketplace_install,
            $crate::commands::marketplace_download,
            $crate::commands::marketplace_upload,
            $crate::commands::marketplace_like,
            $crate::commands::marketplace_my_likes,
            $crate::commands::marketplace_my_packs,
            $crate::commands::marketplace_delete,
            $crate::commands::github_device_flow_start,
            $crate::commands::github_device_flow_poll,
            $crate::commands::github_device_flow_cancel,
            $crate::commands::marketplace_auth_status,
            $crate::commands::cloud_sync_status,
            $crate::commands::cloud_sync_upload,
            $crate::commands::cloud_sync_restore,
            $crate::commands::cloud_sync_delete,
            $crate::commands::cloud_sync_e2ee_status,
            $crate::commands::cloud_sync_e2ee_claim_setup_prompt,
            $crate::commands::cloud_sync_e2ee_prepare_enable,
            $crate::commands::cloud_sync_e2ee_create,
            $crate::commands::cloud_sync_e2ee_unlock,
            $crate::commands::cloud_sync_e2ee_lock,
            $crate::commands::cloud_sync_e2ee_set_enabled,
            $crate::commands::cloud_sync_e2ee_sync_now,
            $crate::commands::cloud_sync_e2ee_cancel,
            $crate::commands::cloud_sync_e2ee_preview_restore,
            $crate::commands::cloud_sync_e2ee_apply_restore,
            $crate::commands::cloud_sync_e2ee_change_password,
            $crate::commands::cloud_sync_e2ee_delete_remote,
            $crate::commands::cloud_sync_e2ee_sign_out,
            $crate::commands::cloud_sync_e2ee_sign_in_with_token,
            $crate::commands::cloud_sync_e2ee_begin_sign_in,
            $crate::commands::cloud_sync_e2ee_poll_sign_in,
            $crate::commands::cloud_sync_e2ee_cancel_sign_in,
            $crate::commands::cloud_sync_e2ee_get_ui_preferences,
            $crate::commands::cloud_sync_e2ee_get_ui_preferences_snapshot,
            $crate::commands::cloud_sync_e2ee_set_ui_preferences_checked,
            $crate::commands::marketplace_logout,
            $crate::commands::list_vocab,
            $crate::commands::add_vocab,
            $crate::commands::add_learned_vocab,
            $crate::commands::remove_vocab,
            $crate::commands::set_vocab_enabled,
            $crate::commands::update_vocab,
            $crate::commands::list_correction_rules,
            $crate::commands::add_correction_rule,
            $crate::commands::remove_correction_rule,
            $crate::commands::set_correction_rule_enabled,
            $crate::commands::list_vocab_presets,
            $crate::commands::save_vocab_presets,
            $crate::commands::start_dictation,
            $crate::commands::stop_dictation,
            $crate::commands::cancel_dictation,
            $crate::commands::qa_window_dismiss,
            $crate::commands::qa_window_set_expanded,
            $crate::commands::qa_get_snapshot,
            $crate::commands::qa_toggle_recording,
            $crate::commands::qa_submit_text,
            $crate::commands::qa_set_edit_instruction_mode,
            $crate::commands::repolish,
            $crate::commands::list_style_packs,
            $crate::commands::create_style_pack_from_template,
            $crate::commands::save_style_pack,
            $crate::commands::set_style_pack_icon,
            $crate::commands::read_style_pack_icon,
            $crate::commands::preview_style_pack_runtime,
            $crate::commands::set_active_style_pack,
            $crate::commands::set_style_pack_enabled,
            $crate::commands::reset_builtin_style_pack,
            $crate::commands::delete_style_pack,
            $crate::commands::import_style_pack_from_zip,
            $crate::commands::export_style_pack_to_zip,
            $crate::commands::set_default_polish_mode,
            $crate::commands::set_style_enabled,
            $crate::commands::check_accessibility_permission,
            $crate::commands::request_accessibility_permission,
            $crate::commands::check_microphone_permission,
            $crate::commands::request_microphone_permission,
            $crate::commands::open_system_settings,
            $crate::commands::trigger_microphone_prompt,
            $crate::commands::export_error_log,
            #[cfg(target_os = "android")]
            $crate::commands::export_error_log_to_downloads,
            $crate::commands::get_update_channel,
            $crate::commands::set_update_channel,
            $crate::commands::fetch_latest_beta_release,
            $crate::commands::app_check_update_with_channel,
            $crate::commands::app_download_and_install_android_update,
            $crate::restart_app,
            $crate::reset_accessibility_permission_and_restart_app,
            $crate::log_client_error,
        ]
    };
}

#[cfg(not(mobile))]
fn run_desktop() {
    // Credential authorization can precede Tauri setup. Keep its diagnostics visible.
    init_file_logger();
    let foundry_local_runtime = Arc::new(asr::local::FoundryLocalRuntime::new());
    let sherpa_onnx_runtime = Arc::new(asr::local::SherpaOnnxRuntime::new());
    #[cfg(target_os = "windows")]
    let coordinator = Arc::new(coordinator::Coordinator::new_with_local_runtimes(
        Arc::clone(&foundry_local_runtime),
        Arc::clone(&sherpa_onnx_runtime),
    ));
    #[cfg(not(target_os = "windows"))]
    let coordinator = Arc::new(coordinator::Coordinator::new());
    let core_backend = coordinator.backend();
    // Runtime effects follow Core startup/recovery
    // in tauri_events::start; pending restore must not mutate the old vault here.
    let builder = tauri::Builder::default();
    // macOS: the capsule must overlay other apps' fullscreen Spaces, which requires a
    // non-activating NSPanel (a plain NSWindow cannot do this even with collectionBehavior —
    // tauri#9556 / #11488). capsule.to_panel() in setup below depends on the panel registry
    // this plugin registers; the plugin is macOS-only.
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    builder
        // Single-instance lock: the second process exits immediately and forwards the
        // activation signal to the running instance's main window. Otherwise two OpenLess
        // copies (e.g. /Applications/ + dev build) each grab the global hotkey, so one key
        // press runs the pipeline in both processes and text gets inserted twice. See issue #50.
        //
        // 第二个进程的 argv 还会转发 CLI 意图给主实例 coordinator；
        // Linux 桌面快捷键由 egui Host 处理。详见 issue #420 / `cli.rs`。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(intent) = cli::parse_cli_intent(&argv) {
                log::info!(
                    "[single-instance] another instance launched with intent={intent:?}, dispatching"
                );
                dispatch_cli_intent(app, intent);
                return;
            }
            // Silent-start mode: a second launch (Win11 "reopen apps after sign-in",
            // autostart double-trigger, or the user clicking the icon again) must not pop
            // the main window either, otherwise start_minimized=true is defeated entirely
            // on Win11. Users open the main window via the tray menu / tray left-click.
            // issue #468.
            if let Some(coordinator) = app
                .try_state::<Arc<coordinator::Coordinator>>()
                .map(|s| Arc::clone(&*s))
            {
                if coordinator.startup_error().is_none()
                    && coordinator.backend().get_preferences().start_minimized {
                    log::info!(
                        "[single-instance] start_minimized=true → skipping show on relaunch"
                    );
                    return;
                }
            }
            log::info!(
                "[single-instance] another instance launched, focusing existing main window"
            );
            show_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        // 桌面开机自启：mac 写 LaunchAgent plist，windows 写
        // HKCU\Software\Microsoft\Windows\CurrentVersion\Run。前端 toggle 直接
        // 调插件 isEnabled / enable / disable，不维持本地 prefs，让 OS 当唯一真相。issue #194。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(coordinator.clone())
        .manage(core_backend.clone())
        .manage(foundry_local_runtime.clone())
        .manage(sherpa_onnx_runtime.clone())
        .manage(commands::MicrophoneMonitorState::new(None))
        .manage(commands::TrayMicrophoneMenuState::new(Vec::new()))
        .manage(TrayMicrophoneDeviceCache(parking_lot::Mutex::new(Vec::new())))
        .setup(move |app| {
            log::info!("=== OpenLess 启动 ===");

            // Position the capsule at the bottom-center of the screen and hide it at
            // startup; the coordinator shows it on demand. Same semantics as Swift
            // `CapsuleWindowController.repositionToBottomCenter`.
            if let Some(capsule) = app.get_webview_window("capsule") {
                // macOS: convert to a non-activating NSPanel, otherwise the capsule cannot
                // overlay other apps' fullscreen Spaces (a plain NSWindow cannot do this
                // with collectionBehavior alone — tauri#9556 / #11488).
                #[cfg(target_os = "macos")]
                {
                    use tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior;
                    use tauri_nspanel::WebviewWindowExt;
                    match capsule.to_panel() {
                        Ok(panel) => {
                            // Non-activating: showing/clicking never activates this app
                            // or switches the current (incl. fullscreen) Space.
                            const NS_NONACTIVATING_PANEL_MASK: i32 = 1 << 7;
                            panel.set_style_mask(NS_NONACTIVATING_PANEL_MASK);
                            // Above the menu bar (24).
                            panel.set_level(25);
                            // Join all Spaces + appear as an auxiliary window over
                            // fullscreen app Spaces.
                            panel.set_collection_behaviour(
                                NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
                                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces,
                            );
                        }
                        Err(e) => log::warn!("[capsule] to_panel failed: {e:?}"),
                    }
                }
                // 纯光效舞台没有任何可点元素（✕/✓ 按钮已移除），而窗口放大到 460×180
                // 盖住屏幕底部中央 —— 必须鼠标穿透，否则会挡住底下应用的点击。
                let cursor_passthrough_ready = true;

                if cursor_passthrough_ready {
                    if let Err(e) = capsule.set_ignore_cursor_events(true) {
                        log::warn!("[capsule] set_ignore_cursor_events failed: {e}");
                    }
                }
                if let Err(e) = position_capsule_bottom_center(&capsule, false) {
                    log::warn!("[capsule] position failed: {e}");
                }
                let _ = capsule.hide();
            }

            // QA / Less Computer windows are lazily created (not eagerly declared in
            // tauri.conf.json): they are built on first use (ensure_qa_window /
            // ensure_less_computer_window), so there is no extra WebKit process while idle
            // — saving 3 resident webviews. Positioning and the QA drag fix are applied in
            // the creation/show paths.

            // Main-window frosted material: NSVisualEffectView on macOS, Mica on Windows.
            // Without this layer, transparent: true makes the window see-through to
            // nothing instead of frosted glass.
            //
            // decorations is decided per-platform at runtime: macOS keeps true for the
            // system traffic lights; Windows disables native chrome so the React
            // WinTitleBar takes over.
            if let Some(main) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{
                        apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                    };
                    if let Err(e) = main.set_decorations(true) {
                        log::warn!("[main] enable native decorations failed: {e}");
                    }
                    if let Err(e) = apply_vibrancy(
                        &main,
                        NSVisualEffectMaterial::HudWindow,
                        Some(NSVisualEffectState::Active),
                        Some(20.0),
                    ) {
                        log::warn!("[main] vibrancy failed: {e}");
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    use window_vibrancy::apply_mica;
                    // Windows uses Tauri decorations:true for the native Win11 title bar /
                    // close button / dragging / rounded corners / resize border. Keep
                    // apply_mica to give the native chrome a frosted material; combined
                    // with WindowChrome's translucent background it lets the sidebar show
                    // through with a glassy feel.
                    if let Err(e) = apply_mica(&main, None) {
                        log::warn!("[main] mica failed: {e}");
                    }
                    // Win11 22H2+: sync the native title bar theme; the frontend calls
                    // set_windows_caption_theme again once ready. Silently no-ops on
                    // older Windows.
                    apply_windows_caption_theme(&main, false);
                }
                // Silent-start switch: prefs.start_minimized = true → don't pop the main
                // window; the user reaches it from the menu bar / tray. Especially useful
                // with autostart to avoid the main window on every login.
                // OPENLESS_SHOW_MAIN_ON_START=1 still forces the old show path (manual
                // dispatch testing / dev) and takes priority over prefs.
                let force_show =
                    std::env::var("OPENLESS_SHOW_MAIN_ON_START").ok().as_deref() == Some("1");
                let suppress_show = !force_show && coordinator.startup_error().is_none()
                    && coordinator.backend().get_preferences().start_minimized;
                if suppress_show {
                    log::info!("[main] start_minimized=true → 跳过初始 show，等用户点托盘");
                } else {
                    if let Err(e) = main.show() {
                        log::warn!("[main] initial show failed: {e}");
                    }
                }
            }

            // Accessibility permission is no longer requested inside setup().
            //
            // Reason: setup() runs before the AppKit event loop is ready, and the XPC
            // communication of AXIsProcessTrustedWithOptions depends on the run loop. On
            // some macOS versions the TCC prompt may not appear yet still be marked as
            // "shown" — subsequent calls from the frontend onboarding never prompt again,
            // and recovery requires System Settings + a reboot, stranding the user on the
            // onboarding page.
            //
            // The frontend onboarding page now triggers it when the user clicks the
            // "enable Accessibility" button, per Apple HIG: "ask after explaining the
            // purpose". Already-authorized users are unaffected (AXIsProcessTrusted
            // returns true and onboarding skips the step).


            // Menu bar icon — same semantics as Swift `MenuBarController`:
            // left click → show/focus the main window; menu has "show main window" and "quit".
            let tray_menu = build_tray_menu(app, &coordinator)?;
            let menu = tray_menu.menu;

            // Matches Swift `StatusBarIcon.swift`: use the full-color AppIcon, **not**
            // template mode (template gets tinted monochrome by macOS → looks like a black
            // square).
            if let Some(icon) = app.default_window_icon() {
                {
                    let state = app.state::<commands::TrayMicrophoneMenuState>();
                    *state.lock() = tray_menu.microphone_items;
                }
                let _tray = TrayIconBuilder::with_id("main-tray")
                    .icon(icon.clone())
                    .icon_as_template(false)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app, event| match event.id.as_ref() {
                        "toggle" => show_main_window(app),
                        "quit" => app.exit(0),
                        id => {
                            if handle_style_tray_menu_event(app, id) {
                                return;
                            }
                            handle_microphone_tray_menu_event(app, id);
                        }
                    })
                    .on_tray_icon_event(move |tray, event| match event {
                        TrayIconEvent::Enter { .. } => {
                            if let Err(err) = refresh_tray_microphone_menu(tray.app_handle()) {
                                log::warn!("[tray] refresh microphone menu on hover failed: {err}");
                            }
                        }
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            ..
                        } => show_main_window(tray.app_handle()),
                        _ => {}
                    })
                    .build(app)?;
                start_tray_microphone_watcher(app.handle().clone());
            } else {
                log::warn!("[startup] default window icon missing; tray icon disabled");
            }

            // Bind recovery effects before Core startup; the Ready handler
            // starts hotkey supervisors once the recovery fence is clear.
            let app_handle = app.handle().clone();
            coordinator.tauri_host().bind(app_handle);
            coordinator.bind_restore_runtime_effects()?;
            if core_backend.ensure_runtime_ready().is_ok() {
                coordinator.sync_capsule_style_from_preferences();
            }
            crate::tauri_events::start(app.handle().clone(), Arc::clone(&core_backend));
            // QA / custom combo hotkeys use `global-hotkey` (Carbon on macOS).
            // Start those after RunEvent::Ready, when the AppKit event loop is live.
            if std::env::var("OPENLESS_SHOW_MAIN_ON_START").ok().as_deref() == Some("1") {
                show_main_window(app.handle());
            }

            // First launch may also carry a CLI flag (the user may run the CLI before
            // double-clicking the .desktop). Dispatch after the coordinator is ready; the
            // GUI still starts normally.
            let first_run_args: Vec<String> = std::env::args().collect();
            if let Some(intent) = cli::parse_cli_intent(&first_run_args) {
                log::info!("[startup] first-run CLI intent={intent:?}, dispatching");
                dispatch_cli_intent(app.handle(), intent);
            }

            Ok(())
        })
        .invoke_handler(app_invoke_handler_desktop!())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::Ready => {
                let coordinator = app.state::<Arc<coordinator::Coordinator>>();
                coordinator.start_hotkey_supervisors_when_ready();
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show_main_window(app),
            RunEvent::WindowEvent { label, event, .. } => {
                if label == "main" {
                    if let tauri::WindowEvent::CloseRequested { ref api, .. } = event {
                        api.prevent_close();
                        hide_main_window(app);
                    }
                }
            }
            RunEvent::Exit => {
                TRAY_MICROPHONE_WATCHER_STOPPING.store(true, Ordering::Relaxed);
                let coordinator = app.state::<Arc<coordinator::Coordinator>>();
                coordinator.stop_hotkey_listener();
                coordinator.stop_qa_hotkey_listener();
                coordinator.stop_selection_polish_hotkey_listener();
                coordinator.stop_coding_agent_hotkey_listener();
                coordinator.stop_combo_hotkey_listener();
                coordinator.stop_translation_hotkey_listener();
                coordinator.stop_switch_style_hotkey_listener();
                coordinator.stop_open_app_hotkey_listener();
                coordinator.stop_quick_note_hotkey_listener();
                coordinator.stop_style_pack_hotkey_listeners();
                let backend = coordinator.backend();
                tauri::async_runtime::spawn(async move {
                    let _ = backend.shutdown().await;
                });
            }
            _ => {}
        });
}

#[cfg(not(mobile))]
struct MicrophoneTrayMenu {
    submenu: Submenu<tauri::Wry>,
    items: Vec<commands::TrayMicrophoneMenuItem>,
}

#[cfg(not(mobile))]
struct StyleTrayMenu {
    submenu: Submenu<tauri::Wry>,
}

#[cfg(not(mobile))]
struct TrayMenu {
    menu: Menu<tauri::Wry>,
    microphone_items: Vec<commands::TrayMicrophoneMenuItem>,
}

#[derive(Debug, Clone, Copy)]
#[cfg(not(mobile))]
struct TrayLabels {
    toggle: &'static str,
    style: &'static str,
    microphone: &'static str,
    default_microphone: &'static str,
    no_microphones: &'static str,
    default_device_suffix: &'static str,
    quit: &'static str,
    raw: &'static str,
    light: &'static str,
    structured: &'static str,
    formal: &'static str,
}

#[cfg(not(mobile))]
impl TrayLabels {
    fn for_locale(locale: &str) -> Self {
        match locale {
            "en" => Self {
                toggle: "Show main window",
                style: "Output style",
                microphone: "Select microphone",
                default_microphone: "System default microphone",
                no_microphones: "No microphones found",
                default_device_suffix: " (System default)",
                quit: "Quit OpenLess",
                raw: "Raw",
                light: "Light polish",
                structured: "Structured",
                formal: "Formal",
            },
            "es" => Self {
                toggle: "Mostrar ventana principal",
                style: "Estilo de salida",
                microphone: "Seleccionar micrófono",
                default_microphone: "Micrófono del sistema",
                no_microphones: "No se encontraron micrófonos",
                default_device_suffix: " (Predeterminado del sistema)",
                quit: "Salir de OpenLess",
                raw: "Texto original",
                light: "Pulido ligero",
                structured: "Estructurado",
                formal: "Formal",
            },
            "fr" => Self {
                toggle: "Afficher la fenêtre principale",
                style: "Style de sortie",
                microphone: "Choisir le microphone",
                default_microphone: "Microphone du système",
                no_microphones: "Aucun microphone détecté",
                default_device_suffix: " (Par défaut du système)",
                quit: "Quitter OpenLess",
                raw: "Texte original",
                light: "Retouche légère",
                structured: "Structuré",
                formal: "Formel",
            },
            "de" => Self {
                toggle: "Hauptfenster anzeigen",
                style: "Ausgabestil",
                microphone: "Mikrofon auswählen",
                default_microphone: "Systemmikrofon",
                no_microphones: "Keine Mikrofone gefunden",
                default_device_suffix: " (Systemstandard)",
                quit: "OpenLess beenden",
                raw: "Originaltext",
                light: "Leichte Überarbeitung",
                structured: "Strukturiert",
                formal: "Formell",
            },
            "zh-TW" => Self {
                toggle: "顯示主視窗",
                style: "輸出風格",
                microphone: "選擇麥克風",
                default_microphone: "系統預設麥克風",
                no_microphones: "找不到麥克風",
                default_device_suffix: "（系統預設）",
                quit: "退出 OpenLess",
                raw: "原文",
                light: "輕度潤色",
                structured: "清晰結構",
                formal: "正式表達",
            },
            "ja" => Self {
                toggle: "メインウィンドウを表示",
                style: "出力スタイル",
                microphone: "マイクを選択",
                default_microphone: "システムのデフォルトマイク",
                no_microphones: "マイクが見つかりません",
                default_device_suffix: "（システムのデフォルト）",
                quit: "OpenLessを終了",
                raw: "原文",
                light: "軽い整文",
                structured: "明確な構造",
                formal: "正式な表現",
            },
            "ko" => Self {
                toggle: "메인 창 표시",
                style: "출력 스타일",
                microphone: "마이크 선택",
                default_microphone: "시스템 기본 마이크",
                no_microphones: "마이크를 찾을 수 없음",
                default_device_suffix: "（시스템 기본）",
                quit: "OpenLess 종료",
                raw: "원문",
                light: "가벼운 정리",
                structured: "명확한 구조",
                formal: "정식 표현",
            },
            _ => Self {
                toggle: "显示主窗口",
                style: "输出风格",
                microphone: "选择麦克风",
                default_microphone: "系统默认麦克风",
                no_microphones: "未发现麦克风",
                default_device_suffix: "（系统默认）",
                quit: "退出 OpenLess",
                raw: "原文",
                light: "轻度润色",
                structured: "清晰结构",
                formal: "正式表达",
            },
        }
    }

    fn style_pack_name(self, mode: PolishMode) -> &'static str {
        match mode {
            PolishMode::Raw => self.raw,
            PolishMode::Light => self.light,
            PolishMode::Structured => self.structured,
            PolishMode::Formal => self.formal,
        }
    }

    fn style_pack_label(self, pack: &StylePack) -> String {
        if pack.kind == StylePackKind::Builtin
            && (pack.name.trim().is_empty()
                || pack.name.trim() == builtin_style_pack_default_name(pack.base_mode))
        {
            return self.style_pack_name(pack.base_mode).to_string();
        }
        if pack.name.trim().is_empty() {
            pack.id.clone()
        } else {
            pack.name.clone()
        }
    }

    fn default_device_label(self, device_name: &str) -> String {
        format!("{device_name}{}", self.default_device_suffix)
    }
}

#[cfg(not(mobile))]
fn builtin_style_pack_default_name(mode: PolishMode) -> &'static str {
    match mode {
        PolishMode::Raw => "原文",
        PolishMode::Light => "轻度润色",
        PolishMode::Structured => "清晰结构",
        PolishMode::Formal => "正式表达",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(not(mobile))]
struct TrayStylePackMenuEntry {
    id: String,
    pack_id: String,
    label: String,
    checked: bool,
}

#[cfg(not(mobile))]
const TRAY_STYLE_PACK_MENU_ID_PREFIX: &str = "style-pack-id-";

fn tray_style_menu_enabled() -> bool {
    #[cfg(all(not(mobile), target_os = "windows"))]
    return true;
    #[cfg(not(all(not(mobile), target_os = "windows")))]
    false
}

#[cfg(not(mobile))]
fn tray_style_pack_menu_id(pack_id: &str) -> String {
    format!("{TRAY_STYLE_PACK_MENU_ID_PREFIX}{pack_id}")
}

#[cfg(not(mobile))]
fn parse_tray_style_pack_menu_id(id: &str) -> Option<&str> {
    let pack_id = id.strip_prefix(TRAY_STYLE_PACK_MENU_ID_PREFIX)?;
    (!pack_id.is_empty()).then_some(pack_id)
}

#[cfg(not(mobile))]
fn tray_style_pack_menu_entries(
    packs: &[StylePack],
    active_style_pack_id: &str,
    labels: TrayLabels,
) -> Vec<TrayStylePackMenuEntry> {
    packs
        .iter()
        .filter(|pack| pack.enabled)
        .map(|pack| TrayStylePackMenuEntry {
            id: tray_style_pack_menu_id(&pack.id),
            pack_id: pack.id.clone(),
            label: labels.style_pack_label(pack),
            checked: pack.id == active_style_pack_id,
        })
        .collect()
}

#[cfg(not(mobile))]
fn resolve_tray_style_pack_id<'a>(id: &'a str, packs: &[StylePack]) -> Option<&'a str> {
    let pack_id = parse_tray_style_pack_menu_id(id)?;
    packs
        .iter()
        .any(|pack| pack.enabled && pack.id == pack_id)
        .then_some(pack_id)
}

#[cfg(not(mobile))]
fn build_tray_menu<M: Manager<tauri::Wry>>(
    app: &M,
    coordinator: &Arc<coordinator::Coordinator>,
) -> tauri::Result<TrayMenu> {
    let locale = app
        .try_state::<Arc<openless_core::OpenLessBackend>>()
        .and_then(|backend| backend.services().remote_input.status().ok())
        .map(|status| status.locale)
        .unwrap_or_else(|| "zh-CN".to_string());
    let labels = TrayLabels::for_locale(&locale);
    let toggle = MenuItemBuilder::with_id("toggle", labels.toggle).build(app)?;
    let microphone_menu = build_microphone_tray_menu(app, coordinator, labels)?;
    let quit = MenuItemBuilder::with_id("quit", labels.quit).build(app)?;
    let mut builder = MenuBuilder::new(app);
    let style_menu = if tray_style_menu_enabled() {
        Some(build_style_tray_menu(app, coordinator, labels)?)
    } else {
        None
    };
    if let Some(style_menu) = &style_menu {
        builder = builder.item(&style_menu.submenu);
    }
    let menu = builder
        .items(&[&toggle, &microphone_menu.submenu, &quit])
        .build()?;
    Ok(TrayMenu {
        menu,
        microphone_items: microphone_menu.items,
    })
}

#[cfg(not(mobile))]
fn build_style_tray_menu<M: Manager<tauri::Wry>>(
    app: &M,
    coordinator: &Arc<coordinator::Coordinator>,
    labels: TrayLabels,
) -> tauri::Result<StyleTrayMenu> {
    let prefs = coordinator.backend().get_preferences();
    let packs = coordinator
        .backend()
        .list_style_packs(&prefs.active_style_pack_id)
        .unwrap_or_else(|err| {
            log::warn!("[tray] list style packs for tray menu failed: {err}");
            Vec::new()
        });
    let mut submenu = SubmenuBuilder::with_id(app, "style", labels.style);
    for entry in tray_style_pack_menu_entries(&packs, &prefs.active_style_pack_id, labels) {
        let item = CheckMenuItemBuilder::with_id(&entry.id, entry.label)
            .checked(entry.checked)
            .build(app)?;
        submenu = submenu.item(&item);
    }
    Ok(StyleTrayMenu {
        submenu: submenu.build()?,
    })
}

#[cfg(not(mobile))]
fn build_microphone_tray_menu<M: Manager<tauri::Wry>>(
    app: &M,
    coordinator: &Arc<coordinator::Coordinator>,
    labels: TrayLabels,
) -> tauri::Result<MicrophoneTrayMenu> {
    let selected = coordinator
        .backend()
        .get_preferences()
        .microphone_device_name;
    let mut items = Vec::new();
    let mut submenu = SubmenuBuilder::with_id(app, "microphone", labels.microphone);
    // CoreAudio device enumeration can block inside AudioUnitSetProperty while AppKit is
    // finishing launch. Tray menus must be built on the main thread, so only consume the
    // cache here; the watcher below owns every potentially blocking enumeration.
    let devices = app.state::<TrayMicrophoneDeviceCache>().0.lock().clone();
    let selected_available =
        selected.trim().is_empty() || devices.iter().any(|device| device.name == selected);

    let default_item = CheckMenuItemBuilder::with_id("mic-default", labels.default_microphone)
        .checked(selected.trim().is_empty() || !selected_available)
        .build(app)?;
    submenu = submenu.item(&default_item);
    items.push(commands::TrayMicrophoneMenuItem {
        id: "mic-default".to_string(),
        device_name: String::new(),
        item: default_item,
    });

    if devices.is_empty() {
        let empty = MenuItemBuilder::with_id("mic-empty", labels.no_microphones)
            .enabled(false)
            .build(app)?;
        submenu = submenu.item(&empty);
    } else {
        for (index, device) in devices.into_iter().enumerate() {
            let id = format!("mic-device-{index}");
            let label = if device.is_default {
                labels.default_device_label(&device.name)
            } else {
                device.name.clone()
            };
            let item = CheckMenuItemBuilder::with_id(&id, label)
                .checked(selected == device.name)
                .build(app)?;
            submenu = submenu.item(&item);
            items.push(commands::TrayMicrophoneMenuItem {
                id,
                device_name: device.name,
                item,
            });
        }
    }

    Ok(MicrophoneTrayMenu {
        submenu: submenu.build()?,
        items,
    })
}

#[cfg(not(mobile))]
pub(crate) fn refresh_tray_microphone_menu(app: &AppHandle) -> tauri::Result<()> {
    let coordinator = app.state::<Arc<coordinator::Coordinator>>();
    let tray_menu = build_tray_menu(app, &coordinator)?;
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(tray_menu.menu))?;
    }
    let state = app.state::<commands::TrayMicrophoneMenuState>();
    *state.lock() = tray_menu.microphone_items;
    Ok(())
}

#[cfg(not(mobile))]
fn microphone_devices_with_signature(
) -> Option<(Vec<recorder::MicrophoneDevice>, Vec<(String, bool)>)> {
    match recorder::list_input_devices() {
        Ok(devices) => {
            let signature = devices
                .iter()
                .map(|device| (device.name.clone(), device.is_default))
                .collect();
            Some((devices, signature))
        }
        Err(err) => {
            log::warn!("[tray] watch microphone devices failed: {err}");
            None
        }
    }
}

/// Enumerate devices off the main thread, update the shared cache, then rebuild the tray on
/// AppKit's main thread only when the device signature changed.
#[cfg(not(mobile))]
fn refresh_microphone_cache_if_changed(
    app: &AppHandle,
    last_signature: &parking_lot::Mutex<Option<Vec<(String, bool)>>>,
) {
    let Some((devices, signature)) = microphone_devices_with_signature() else {
        return;
    };
    {
        let mut guard = last_signature.lock();
        if guard.as_ref() == Some(&signature) {
            return;
        }
        *guard = Some(signature);
    }
    *app.state::<TrayMicrophoneDeviceCache>().0.lock() = devices;
    let refresh_app = app.clone();
    if let Err(err) = app.run_on_main_thread(move || refresh_microphone_on_main(&refresh_app)) {
        log::warn!("[tray] dispatch microphone cache refresh failed: {err}");
    }
}

/// Refreshes the tray microphone submenu on the main thread and notifies the frontend.
/// Shared finalization path for the OS-native device-change callback and the slow polling
/// fallback. Call on the main thread, or after being dispatched via `run_on_main_thread`.
#[cfg(not(mobile))]
fn refresh_microphone_on_main(app: &AppHandle) {
    if let Err(err) = refresh_tray_microphone_menu(app) {
        log::warn!("[tray] refresh microphone menu after device change failed: {err}");
    }
    tauri_events::publish(
        app,
        None,
        openless_core::BackendEventKind::MicrophoneDevicesChanged,
    );
}

/// Device-change debounce closure: called by OS-native notification callbacks (macOS
/// CoreAudio / Windows MMDevice). The OS callback only schedules a background enumeration
/// task; devices are never enumerated directly on the AppKit main thread or the
/// CoreAudio/COM notification thread. Concurrent notifications are coalesced via
/// `refresh_in_flight`, and signature debouncing avoids redundant menu refreshes.
#[cfg(not(mobile))]
fn make_microphone_change_handler(app: AppHandle) -> impl Fn() + Send + Sync + 'static {
    let last_signature = Arc::new(parking_lot::Mutex::new(None));
    let refresh_in_flight = Arc::new(AtomicBool::new(false));
    move || {
        if refresh_in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        let refresh_app = app.clone();
        let refresh_signature = Arc::clone(&last_signature);
        let refresh_flag = Arc::clone(&refresh_in_flight);
        if let Err(err) = std::thread::Builder::new()
            .name("openless-tray-mic-event".into())
            .spawn(move || {
                refresh_microphone_cache_if_changed(&refresh_app, &refresh_signature);
                refresh_flag.store(false, Ordering::Release);
            })
        {
            refresh_in_flight.store(false, Ordering::Release);
            log::warn!("[tray] start microphone event refresh failed: {err}");
        }
    }
}

#[cfg(not(mobile))]
fn start_tray_microphone_watcher(app: AppHandle) {
    TRAY_MICROPHONE_WATCHER_STOPPING.store(false, Ordering::Relaxed);

    // 1) OS 原生设备变更通知（issue #470 的最优方案）：空闲零唤醒。
    //    macOS → CoreAudio AudioObjectAddPropertyListener；Windows → IMMNotificationClient。
    //    未提供原生路径的平台返回 false，纯靠下面的慢速兜底。
    //    注册失败（OSStatus≠0 / RegisterEndpoint Err）只 warn，不 panic——兜底轮询保证
    //    三平台都「永远能检测到设备」。
    let native_registered = device_watch::spawn_native_watcher(
        app.clone(),
        make_microphone_change_handler(app.clone()),
    );
    if native_registered {
        log::info!("[tray] OS native microphone device watcher registered");
    } else {
        log::info!(
            "[tray] no OS native microphone device watcher (unsupported platform or registration failed); relying on slow poll fallback"
        );
    }

    // 2) All-platform slow fallback: unconditional 60s polling, reusing the signature
    //    debounce (unchanged signature → continue, zero side effects). Guarantees device
    //    changes are eventually detected when native notifications fail; when they work,
    //    it is just a very-low-frequency safety net that almost never actually refreshes.
    if let Err(err) = std::thread::Builder::new()
        .name("openless-tray-mic-poll".into())
        .spawn(move || {
            let last_signature = parking_lot::Mutex::new(None);
            // Populate the initially empty tray cache without blocking AppKit startup.
            refresh_microphone_cache_if_changed(&app, &last_signature);
            while !TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                // 60s (not 10s): native notifications do real-time detection, this thread
                // is only a fallback, so push it to 60s to further cut idle wakeups.
                // Sleeping in 1s slices lets the exit flag take effect within 1s, avoiding
                // a long thread hang on exit.
                for _ in 0..60 {
                    if TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                if TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                    break;
                }
                refresh_microphone_cache_if_changed(&app, &last_signature);
            }
        })
    {
        log::warn!("[tray] start microphone poll fallback failed: {err}");
    }
}

#[cfg(not(mobile))]
fn handle_microphone_tray_menu_event(app: &AppHandle, id: &str) {
    let tray_items = app.state::<commands::TrayMicrophoneMenuState>();
    let items = tray_items.lock();
    let Some(selected) = items.iter().find(|item| item.id == id) else {
        return;
    };

    let coord = app.state::<Arc<coordinator::Coordinator>>();
    if let Err(err) = coord
        .backend()
        .select_microphone_device(selected.device_name.clone())
    {
        log::warn!("[tray] save microphone preference failed: {err}");
        return;
    }

    commands::sync_tray_microphone_selection(&items, &selected.device_name);
}

#[cfg(not(mobile))]
fn handle_style_tray_menu_event(app: &AppHandle, id: &str) -> bool {
    let Some(pack_id) = parse_tray_style_pack_menu_id(id) else {
        return false;
    };
    let coord = app.state::<Arc<coordinator::Coordinator>>();
    let prefs = coord.backend().get_preferences();
    let packs = match coord
        .backend()
        .list_style_packs(&prefs.active_style_pack_id)
    {
        Ok(packs) => packs,
        Err(err) => {
            log::warn!("[tray] validate style pack tray item failed: {err}");
            return true;
        }
    };
    if resolve_tray_style_pack_id(id, &packs).is_none() {
        log::warn!("[tray] ignore stale or disabled style pack tray item id={pack_id}");
        return true;
    }
    if let Err(err) = commands::activate_style_pack_by_id(&coord, app, pack_id) {
        log::warn!("[tray] activate style pack from tray failed: {err}");
        return true;
    }
    if let Err(err) = refresh_tray_microphone_menu(app) {
        log::warn!("[tray] refresh style menu after polish mode change failed: {err}");
    }
    true
}

#[cfg(mobile)]
pub(crate) fn refresh_tray_microphone_menu(_app: &AppHandle) -> tauri::Result<()> {
    Ok(())
}

/// Win11 22H2+ (Build 22621+) syncs the native title bar immersive dark / caption / text /
/// border colors. DwmSetWindowAttribute errors on older Windows: warn only, never block startup.
#[cfg(target_os = "windows")]
fn apply_windows_caption_theme<R: Runtime>(window: &tauri::WebviewWindow<R>, dark: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
    };

    let handle = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle,
        Ok(other) => {
            log::warn!("[main] unexpected raw window handle for caption theme: {other:?}");
            return;
        }
        Err(e) => {
            log::warn!("[main] read raw window handle for caption theme failed: {e}");
            return;
        }
    };
    let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);

    // COLORREF 0x00BBGGRR — light matches the WindowChrome glass start color rgb(245,245,247);
    // dark matches tokens.css --ol-surface (#141922) / --ol-ink (#f4f7fb) / --ol-surface-2 (#1a202b).
    let immersive_dark: i32 = i32::from(dark);
    let caption_color: u32 = if dark { 0x0022_1914 } else { 0x00F7_F5F5 };
    let text_color: u32 = if dark { 0x00FB_F7F4 } else { 0x002A_170F };
    let border_color: u32 = if dark { 0x002B_201A } else { 0x00E8_E8E8 };

    unsafe {
        set_dwm_window_attribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &immersive_dark,
            "immersive dark mode",
        );
        set_dwm_window_attribute(hwnd, DWMWA_CAPTION_COLOR, &caption_color, "caption color");
        set_dwm_window_attribute(hwnd, DWMWA_TEXT_COLOR, &text_color, "text color");
        set_dwm_window_attribute(hwnd, DWMWA_BORDER_COLOR, &border_color, "border color");
    }
}

#[cfg(target_os = "windows")]
unsafe fn set_dwm_window_attribute<T>(
    hwnd: windows::Win32::Foundation::HWND,
    attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    value: &T,
    label: &str,
) {
    use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    if let Err(e) = DwmSetWindowAttribute(
        hwnd,
        attribute,
        value as *const _ as *const core::ffi::c_void,
        std::mem::size_of_val(value) as u32,
    ) {
        log::warn!("[main] set {label} failed (likely pre-22H2 Win): {e}");
    }
}

/// Syncs the main window's native title bar on frontend theme change; no-op off Windows.
#[tauri::command]
fn set_windows_caption_theme(app: AppHandle, dark: bool) {
    #[cfg(target_os = "windows")]
    if let Some(main) = app.get_webview_window("main") {
        apply_windows_caption_theme(&main, dark);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, dark);
    }
}

#[tauri::command]
fn restart_app(app: AppHandle) {
    prepare_for_restart();
    #[cfg(target_os = "macos")]
    reset_tcc_for_beta_restart();
    app.restart();
}

#[tauri::command]
fn reset_accessibility_permission_and_restart_app(app: AppHandle) {
    prepare_for_restart();
    #[cfg(target_os = "macos")]
    reset_tcc_service_for_restart("Accessibility", "accessibility recovery");
    app.restart();
}

fn prepare_for_restart() {
    // macOS: auto-updates leave the newly installed .app with com.apple.quarantine (no
    // matter how the Tauri updater unpacks, the download stream goes through LaunchServices
    // and the output may still carry the xattr). Without stripping it, Gatekeeper blocks
    // the relaunch with "OpenLess is damaged / from an unidentified developer", forcing the
    // user to run xattr -cr manually — violating "auto-update must be zero-friction".
    //
    // Clear the xattr once, blocking, before restart. Tolerate failures (bad PATH, missing
    // xattr, read-only disk, etc.); never block the restart itself.
    #[cfg(target_os = "macos")]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundle) = exe
            .ancestors()
            .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
        {
            let _ = std::process::Command::new("/usr/bin/xattr")
                .arg("-cr")
                .arg(bundle)
                .status();
            log::info!("[updater] stripped xattr on {:?} before restart", bundle);
        }
    }
}

/// Forwards critical frontend errors (e.g. auto-update install failures) to the Rust file
/// log (openless.log). The webview's console.error never reaches openless.log, so this
/// dedicated IPC exists — after the user "exports logs" we get the real update-failure cause.
#[tauri::command]
fn log_client_error(message: String) {
    // message is frontend-webview controlled and may be long or contain newlines (log
    // forgery). Fold newlines into spaces first, then truncate on a UTF-8 char boundary,
    // so one entry can't blow up the log size or its format.
    const MAX_LEN: usize = 2048;
    let mut sanitized = message.replace(['\n', '\r'], " ");
    if sanitized.len() > MAX_LEN {
        let mut end = MAX_LEN;
        while !sanitized.is_char_boundary(end) {
            end -= 1;
        }
        sanitized.truncate(end);
        sanitized.push_str("…(truncated)");
    }
    log::error!("[client] {sanitized}");
}

#[cfg(target_os = "macos")]
fn reset_tcc_for_beta_restart() {
    if !is_beta_build() {
        log::info!("[updater] skipping TCC reset before stable restart");
        return;
    }

    // Beta builds are currently ad-hoc signed. Their code hash changes across builds, so
    // old TCC rows can leave System Settings checked while AXIsProcessTrusted() is false.
    reset_tcc_service_for_restart("Accessibility", "beta ad-hoc identity refresh");
    reset_tcc_service_for_restart("Microphone", "beta ad-hoc identity refresh");
}

#[cfg(target_os = "macos")]
fn is_beta_build() -> bool {
    env!("CARGO_PKG_VERSION").contains('-')
}

#[cfg(target_os = "macos")]
fn reset_tcc_service_for_restart(service: &str, reason: &str) {
    match std::process::Command::new("/usr/bin/tccutil")
        .args(["reset", service, OPENLESS_BUNDLE_ID])
        .status()
    {
        Ok(status) if status.success() => {
            log::info!("[tcc] reset {service} before restart ({reason})");
        }
        Ok(status) => {
            log::warn!("[tcc] reset {service} before restart ({reason}) exited with {status}");
        }
        Err(e) => {
            log::warn!("[tcc] reset {service} before restart ({reason}) failed: {e}");
        }
    }
}

/// Writes logs to both stderr and ~/Library/Logs/OpenLess/openless.log (matches Swift `Log.swift`).
pub(crate) fn init_file_logger() {
    use simplelog::{
        ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, TermLogger, TerminalMode,
        WriteLogger,
    };
    let log_dir = log_dir_path();
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "[logger] WARN create log dir failed path={}: {e}",
            log_dir.display()
        );
    }
    let log_file = log_dir.join("openless.log");
    if let Err(e) = rotate_log_if_too_large(&log_file) {
        eprintln!("[logger] WARN 日志轮转失败: {e}");
    }
    let config = ConfigBuilder::new().set_time_format_rfc3339().build();
    let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> = vec![TermLogger::new(
        LevelFilter::Info,
        config.clone(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )];
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        Ok(file) => {
            loggers.push(WriteLogger::new(LevelFilter::Info, config, file));
            eprintln!("[logger] file logger ready path={}", log_file.display());
        }
        Err(e) => {
            eprintln!(
                "[logger] ERROR open log file failed path={}: {e}",
                log_file.display()
            );
        }
    }
    let _ = CombinedLogger::init(loggers);
}

fn rotate_log_if_too_large(path: &std::path::Path) -> std::io::Result<()> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() <= LOG_ROTATE_LIMIT_BYTES {
        return Ok(());
    }

    let archive = path.with_file_name("openless.log.1");
    match std::fs::remove_file(&archive) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::fs::rename(path, archive)
}

pub fn log_dir_path() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home)
                .join("Library")
                .join("Logs")
                .join("OpenLess");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return std::path::PathBuf::from(local)
                .join("OpenLess")
                .join("Logs");
        }
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("OpenLess")
                .join("logs");
        }
    }
    #[cfg(target_os = "android")]
    {
        // Prefer cached JNI filesDir/logs; never use /data/local/tmp.
        if let Ok(dir) = crate::persistence::android_log_dir() {
            return dir;
        }
        eprintln!("[logger] ERROR android_log_dir unavailable; file logging disabled");
        return std::path::PathBuf::from("/__openless_android_log_uninitialized__");
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::temp_dir().join("OpenLess")
    }
}

pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    activate_window_mode(app);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        #[cfg(not(mobile))]
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    activate_app(app);
}

/// Routes a CLI intent to the shared Core or the QA host still owned by Tauri. Two entry
/// points share this: 1. first launch (end of lib.rs setup); 2. the single-instance
/// callback (argv forwarded from the intercepted second process).
///
/// Async actions spawn on the Tauri runtime and never block the callback thread:
/// - ToggleDictation enters the shared Core facade; CancelDictation releases the Less Host
///   capture first, then cancels Core
/// - ToggleQa forwards to handle_qa_hotkey_pressed (same semantics as pressing the QA hotkey)
fn dispatch_cli_intent<R: Runtime>(app: &AppHandle<R>, intent: cli::CliIntent) {
    match intent {
        cli::CliIntent::ToggleDictation => {
            let backend = app
                .try_state::<Arc<openless_core::OpenLessBackend>>()
                .map(|state| Arc::clone(&*state));
            let Some(backend) = backend else {
                log::warn!("[cli] core backend not yet managed; dropping intent={intent:?}");
                return;
            };
            tauri::async_runtime::spawn(async move {
                log::info!("[cli] dispatching intent={intent:?} to core backend");
                if !backend.snapshot().running {
                    if let Err(error) = backend.start().await {
                        log::warn!("[cli] core backend start failed: {error}");
                        return;
                    }
                }
                if let Err(error) = backend.dispatch_cli_intent(intent).await {
                    log::warn!("[cli] core intent failed: {error}");
                }
            });
        }
        cli::CliIntent::CancelDictation => {
            let coordinator = app
                .try_state::<Arc<coordinator::Coordinator>>()
                .map(|state| Arc::clone(&*state));
            let Some(coordinator) = coordinator else {
                log::warn!("[cli] coordinator not yet managed; dropping cancel intent");
                return;
            };
            tauri::async_runtime::spawn(async move {
                if let Err(error) = coordinator.cancel_dictation_from_cli().await {
                    log::warn!("[cli] cancel failed: {error}");
                }
            });
        }
        cli::CliIntent::ToggleQa => {
            let coordinator = app
                .try_state::<Arc<coordinator::Coordinator>>()
                .map(|state| Arc::clone(&*state));
            let Some(coordinator) = coordinator else {
                log::warn!("[cli] coordinator not yet managed; dropping QA intent");
                return;
            };
            let coord = Arc::clone(&coordinator);
            tauri::async_runtime::spawn(async move {
                log::info!("[cli] toggle-qa: dispatching to qa hotkey handler");
                coord.cli_toggle_qa_panel().await;
            });
        }
    }
}

pub(crate) fn request_microphone_from_foreground<R: Runtime>(
    app: &AppHandle<R>,
) -> permissions::PermissionStatus {
    show_main_window(app);
    wait_for_app_activation(app);
    permissions::request_microphone()
}

fn hide_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    activate_menu_bar_mode(app);
}

#[cfg(target_os = "macos")]
fn activate_window_mode<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let _ = app.set_dock_visibility(true);
    let _ = app.show();
}

#[cfg(not(target_os = "macos"))]
fn activate_window_mode<R: Runtime>(_app: &AppHandle<R>) {}

#[cfg(target_os = "macos")]
fn activate_menu_bar_mode<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    let _ = app.set_dock_visibility(false);
}

#[cfg(not(target_os = "macos"))]
fn activate_menu_bar_mode<R: Runtime>(_app: &AppHandle<R>) {}

#[cfg(target_os = "macos")]
fn activate_app<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.run_on_main_thread(|| {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject, Bool};

        unsafe {
            let Some(cls) = AnyClass::get("NSApplication") else {
                return;
            };
            let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
            if !ns_app.is_null() {
                let _: () = msg_send![ns_app, activateIgnoringOtherApps: Bool::YES];
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn activate_app<R: Runtime>(_app: &AppHandle<R>) {}

/// Called after showing the capsule: if OpenLess is already the frontmost app, restore main
/// window focus via makeKeyWindow. Does not call NSApp.activate or steal focus from other
/// apps, per the CLAUDE.md constraint.
#[cfg(target_os = "macos")]
pub(crate) fn restore_main_window_key_if_active<R: Runtime>(app: &AppHandle<R>) {
    let main = app.get_webview_window("main");
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject, Bool};
        unsafe {
            let Some(cls) = AnyClass::get("NSApplication") else {
                return;
            };
            let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
            if ns_app.is_null() {
                return;
            }
            let is_active: Bool = msg_send![ns_app, isActive];
            if !is_active.as_bool() {
                return;
            }
            let Some(main) = main else {
                return;
            };
            match main.ns_window() {
                Ok(handle) => {
                    let main_win = handle as *mut AnyObject;
                    if !main_win.is_null() {
                        let _: () = msg_send![main_win, makeKeyWindow];
                    }
                }
                Err(e) => log::warn!("[main] ns_window unavailable for key restore: {e}"),
            };
        }
    });
}

#[cfg(target_os = "macos")]
fn wait_for_app_activation<R: Runtime>(app: &AppHandle<R>) {
    let (tx, rx) = mpsc::channel();
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject, Bool};

        unsafe {
            let Some(cls) = AnyClass::get("NSApplication") else {
                let _ = tx.send(());
                return;
            };
            let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
            if !ns_app.is_null() {
                let _: () = msg_send![ns_app, activateIgnoringOtherApps: Bool::YES];
            }
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_millis(800));
    std::thread::sleep(Duration::from_millis(150));
}

#[cfg(not(target_os = "macos"))]
fn wait_for_app_activation<R: Runtime>(_app: &AppHandle<R>) {}

/// QA starts as a small composer and grows downwards after a question.
const QA_WINDOW_WIDTH: f64 = 480.0;
const QA_WINDOW_HEIGHT: f64 = 80.0;
const QA_WINDOW_EXPANDED_HEIGHT: f64 = 560.0;
/// Gap between the capsule and the QA window; matches the design spec.
const QA_WINDOW_GAP_TO_CAPSULE: f64 = 8.0;
/// Bottom margin reserved for the macOS Dock (same source as the capsule).

#[derive(Clone, Copy, Debug, PartialEq)]
struct LogicalMonitorFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn logical_monitor_frame(
    physical_x: i32,
    physical_y: i32,
    physical_width: u32,
    physical_height: u32,
    scale: f64,
) -> LogicalMonitorFrame {
    let scale = scale.max(0.1);
    LogicalMonitorFrame {
        x: physical_x as f64 / scale,
        y: physical_y as f64 / scale,
        width: physical_width as f64 / scale,
        height: physical_height as f64 / scale,
    }
}

fn bottom_center_position(
    frame: LogicalMonitorFrame,
    window_width: f64,
    window_height: f64,
    bottom_offset: f64,
) -> (f64, f64) {
    let x = frame.x + ((frame.width - window_width) / 2.0).max(0.0);
    let y = frame.y + (frame.height - bottom_offset - window_height).max(0.0);
    (x, y)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn frame_contains_point(frame: LogicalMonitorFrame, x: f64, y: f64) -> bool {
    x >= frame.x && x < frame.x + frame.width && y >= frame.y && y < frame.y + frame.height
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn frame_distance_to_point_squared(frame: LogicalMonitorFrame, x: f64, y: f64) -> f64 {
    let nearest_x = x.clamp(frame.x, frame.x + frame.width);
    let nearest_y = y.clamp(frame.y, frame.y + frame.height);
    let dx = x - nearest_x;
    let dy = y - nearest_y;
    dx * dx + dy * dy
}

/// Snapshot of the capsule's target monitor: physical rect + DPI scale.
///
/// On macOS it is derived from the focused input / caret position and shared by actual
/// positioning and the capsule layout cache. Tauri monitor physical coordinates serve as
/// the stable key; conversion to logical coordinates happens only right before
/// set_position, avoiding doubled window sizes on Retina.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CapsuleTargetMonitor {
    pub(crate) physical_x: i32,
    pub(crate) physical_y: i32,
    pub(crate) physical_width: u32,
    pub(crate) physical_height: u32,
    pub(crate) work_x: i32,
    pub(crate) work_y: i32,
    pub(crate) work_width: u32,
    pub(crate) work_height: u32,
    pub(crate) scale: f64,
}

#[cfg(target_os = "macos")]
impl CapsuleTargetMonitor {
    fn logical_frame(self) -> LogicalMonitorFrame {
        logical_monitor_frame(
            self.physical_x,
            self.physical_y,
            self.physical_width,
            self.physical_height,
            self.scale,
        )
    }

    fn logical_work_area(self) -> LogicalMonitorFrame {
        logical_monitor_frame(
            self.work_x,
            self.work_y,
            self.work_width,
            self.work_height,
            self.scale,
        )
    }
}

/// macOS: decides which monitor the capsule goes on.
///
/// Follows the **screen under the mouse cursor** — the user's action/attention focus and
/// the only signal that is always available and permission-free, reliable across multiple
/// monitors and Spaces. Only when the cursor can't be read (shouldn't happen) does it fall
/// back to the AX focused-input/caret position.
///
/// Key: never use the capsule window's own current_monitor — while hidden it still points
/// at the screen it last appeared on, so with multiple monitors the cache would wrongly
/// conclude "no move needed" and lock the capsule to the first screen. Screen selection
/// first looks for the screen containing the point; if the point briefly falls outside all
/// screens it falls back to the nearest screen, avoiding total invisibility from virtual
/// desktop negative coordinates / screen-arrangement edges. Positioning and the layout
/// dedup cache share this function — both must look at the same screen.
#[cfg(target_os = "macos")]
pub(crate) fn capsule_target_monitor<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Option<CapsuleTargetMonitor> {
    let (x, y) = macos_mouse_cursor_point().or_else(macos_focused_input_anchor_point)?;
    monitor_for_anchor_point(window, x, y)
}

/// Picks, in Tauri's monitor coordinate system, the monitor containing the logical point
/// `(x, y)`; falls back to the nearest monitor when the point is outside all screens.
/// Same coordinate space as AX / CGEvent's global display space (top-left origin, points).
#[cfg(target_os = "macos")]
fn monitor_for_anchor_point<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    x: f64,
    y: f64,
) -> Option<CapsuleTargetMonitor> {
    let monitors = window.available_monitors().ok()?;
    pick_monitor_for_anchor_point(
        monitors.into_iter().map(|monitor| CapsuleTargetMonitor {
            physical_x: monitor.position().x,
            physical_y: monitor.position().y,
            physical_width: monitor.size().width,
            physical_height: monitor.size().height,
            work_x: monitor.work_area().position.x,
            work_y: monitor.work_area().position.y,
            work_width: monitor.work_area().size.width,
            work_height: monitor.work_area().size.height,
            scale: monitor.scale_factor(),
        }),
        x,
        y,
    )
}

/// Pure screen-picking logic (no Tauri / AppKit, so multi-monitor layouts are unit-testable):
/// return the screen containing the point, else the nearest screen.
#[cfg(target_os = "macos")]
fn pick_monitor_for_anchor_point(
    monitors: impl IntoIterator<Item = CapsuleTargetMonitor>,
    x: f64,
    y: f64,
) -> Option<CapsuleTargetMonitor> {
    let mut nearest: Option<(f64, CapsuleTargetMonitor)> = None;

    for target in monitors {
        let frame = target.logical_frame();
        if frame_contains_point(frame, x, y) {
            return Some(target);
        }
        let distance = frame_distance_to_point_squared(frame, x, y);
        match nearest {
            Some((best, _)) if best <= distance => {}
            _ => nearest = Some((distance, target)),
        }
    }

    nearest.map(|(_, target)| target)
}

/// Which monitor a floating window (QA / Less Computer panel / glow outline) goes on.
///
/// macOS uses the same signal as the capsule — the screen under the mouse cursor (see
/// [`capsule_target_monitor`]) — so floating windows always appear on the same screen as
/// the capsule. Other platforms lack that path and keep using the window's own
/// `current_monitor`.
///
/// Key: **never** just ask the floating window's own `current_monitor`. These windows are
/// lazily created and stay hidden; while hidden it points at the screen they last appeared
/// on, and their first-ever appearance is on the system default (primary) screen — so every
/// show computes the primary screen and places them back there, permanently pinned to the
/// primary display and split from the capsule on multi-monitor setups. The capsule hit and
/// fixed this same pitfall (see the `capsule_target_monitor` comment); the three floating
/// windows missed it, fixed here.
fn floating_window_monitor_frame<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<Option<LogicalMonitorFrame>> {
    #[cfg(target_os = "macos")]
    {
        if let Some(monitor) = capsule_target_monitor(window) {
            return Ok(Some(monitor.logical_frame()));
        }
    }
    let Some(monitor) = window.current_monitor()? else {
        return Ok(None);
    };
    let size = monitor.size();
    let pos = monitor.position();
    Ok(Some(logical_monitor_frame(
        pos.x,
        pos.y,
        size.width,
        size.height,
        monitor.scale_factor(),
    )))
}

/// First presentation may follow the pointer; resizing an existing chat stays
/// on its own monitor. Work areas exclude the Dock/menu bar/taskbar.
fn chat_window_work_area<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    initial: bool,
) -> tauri::Result<Option<(LogicalMonitorFrame, f64)>> {
    #[cfg(target_os = "macos")]
    if initial {
        if let Some(target) = capsule_target_monitor(window) {
            return Ok(Some((target.logical_work_area(), target.scale.max(0.1))));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = initial;
    let Some(monitor) = window.current_monitor()? else {
        return Ok(None);
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor().max(0.1);
    Ok(Some((
        logical_monitor_frame(
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
            scale,
        ),
        scale,
    )))
}

#[cfg(target_os = "macos")]
fn macos_mouse_cursor_point() -> Option<(f64, f64)> {
    macos_capsule_ax::mouse_cursor_point()
}

#[cfg(target_os = "macos")]
fn macos_focused_input_anchor_point() -> Option<(f64, f64)> {
    macos_capsule_ax::focused_input_anchor_point()
}

#[cfg(target_os = "macos")]
mod macos_capsule_ax {
    use std::ffi::{c_void, CStr};
    use std::os::raw::c_char;

    #[repr(C)]
    struct OpaqueAxRef(c_void);
    type AxUiElementRef = *mut OpaqueAxRef;
    type CFStringRef = *const c_void;
    type CFTypeRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type AxError = i32;
    type AxValueRef = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    const AX_ERROR_SUCCESS: AxError = 0;
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_AX_VALUE_CG_POINT_TYPE: i32 = 1;
    const K_AX_VALUE_CG_SIZE_TYPE: i32 = 2;
    const K_AX_VALUE_CG_RECT_TYPE: i32 = 3;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AxUiElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AxError;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: AxUiElementRef,
            parameterized_attribute: CFStringRef,
            parameter: CFTypeRef,
            value: *mut CFTypeRef,
        ) -> AxError;
        fn AXValueGetValue(value: AxValueRef, value_type: i32, out: *mut c_void) -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFStringCreateWithCString(
            allocator: CFAllocatorRef,
            cstr: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
    }

    pub(super) fn focused_input_anchor_point() -> Option<(f64, f64)> {
        unsafe {
            let focused = focused_element()?;
            let rect = caret_rect(focused).or_else(|| element_rect(focused));
            CFRelease(focused as CFTypeRef);
            let rect = rect?;
            let width = rect.size.width.max(1.0);
            let height = rect.size.height.max(1.0);
            Some((rect.origin.x + width / 2.0, rect.origin.y + height / 2.0))
        }
    }

    type CGEventRef = *const c_void;
    type CGEventSourceRef = *const c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
        fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    }

    /// Current mouse cursor position in the "global display coordinate space" (top-left
    /// origin, points). Identical to the AX caret coordinate space, so it can be compared
    /// directly against `logical_frame` for screen picking. `CGEventGetLocation` is always
    /// available and needs no permissions, making it the preferred signal for the capsule's
    /// screen following.
    pub(super) fn mouse_cursor_point() -> Option<(f64, f64)> {
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let point = CGEventGetLocation(event);
            CFRelease(event as CFTypeRef);
            Some((point.x, point.y))
        }
    }

    unsafe fn cfstring_from_static(bytes_with_nul: &[u8]) -> Option<CFStringRef> {
        let cstr = CStr::from_bytes_with_nul(bytes_with_nul).ok()?;
        let s =
            CFStringCreateWithCString(std::ptr::null(), cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if s.is_null() {
            None
        } else {
            Some(s)
        }
    }

    unsafe fn focused_element() -> Option<AxUiElementRef> {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let Some(focused_attr) = cfstring_from_static(b"AXFocusedUIElement\0") else {
            CFRelease(system as CFTypeRef);
            return None;
        };
        let mut focused: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(system, focused_attr, &mut focused);
        CFRelease(system as CFTypeRef);
        CFRelease(focused_attr);
        if err != AX_ERROR_SUCCESS || focused.is_null() {
            None
        } else {
            Some(focused as AxUiElementRef)
        }
    }

    unsafe fn caret_rect(focused: AxUiElementRef) -> Option<CGRect> {
        let range_attr = cfstring_from_static(b"AXSelectedTextRange\0")?;
        let Some(bounds_attr) = cfstring_from_static(b"AXBoundsForRange\0") else {
            CFRelease(range_attr);
            return None;
        };

        let mut range_value: CFTypeRef = std::ptr::null();
        let range_err = AXUIElementCopyAttributeValue(focused, range_attr, &mut range_value);
        CFRelease(range_attr);
        if range_err != AX_ERROR_SUCCESS || range_value.is_null() {
            CFRelease(bounds_attr);
            return None;
        }

        let mut bounds_value: CFTypeRef = std::ptr::null();
        let bounds_err = AXUIElementCopyParameterizedAttributeValue(
            focused,
            bounds_attr,
            range_value,
            &mut bounds_value,
        );
        CFRelease(bounds_attr);
        CFRelease(range_value);
        if bounds_err != AX_ERROR_SUCCESS || bounds_value.is_null() {
            return None;
        }

        let mut rect = CGRect::default();
        let ok = AXValueGetValue(
            bounds_value as AxValueRef,
            K_AX_VALUE_CG_RECT_TYPE,
            &mut rect as *mut _ as *mut c_void,
        );
        CFRelease(bounds_value);
        (ok != 0).then_some(rect)
    }

    unsafe fn element_rect(focused: AxUiElementRef) -> Option<CGRect> {
        let position_attr = cfstring_from_static(b"AXPosition\0")?;
        let Some(size_attr) = cfstring_from_static(b"AXSize\0") else {
            CFRelease(position_attr);
            return None;
        };

        let mut position_value: CFTypeRef = std::ptr::null();
        let position_err =
            AXUIElementCopyAttributeValue(focused, position_attr, &mut position_value);
        CFRelease(position_attr);
        if position_err != AX_ERROR_SUCCESS || position_value.is_null() {
            CFRelease(size_attr);
            return None;
        }

        let mut point = CGPoint::default();
        let point_ok = AXValueGetValue(
            position_value as AxValueRef,
            K_AX_VALUE_CG_POINT_TYPE,
            &mut point as *mut _ as *mut c_void,
        );
        CFRelease(position_value);
        if point_ok == 0 {
            CFRelease(size_attr);
            return None;
        }

        let mut size_value: CFTypeRef = std::ptr::null();
        let size_err = AXUIElementCopyAttributeValue(focused, size_attr, &mut size_value);
        CFRelease(size_attr);
        if size_err != AX_ERROR_SUCCESS || size_value.is_null() {
            return Some(CGRect {
                origin: point,
                size: CGSize {
                    width: 1.0,
                    height: 1.0,
                },
            });
        }

        let mut size = CGSize::default();
        let size_ok = AXValueGetValue(
            size_value as AxValueRef,
            K_AX_VALUE_CG_SIZE_TYPE,
            &mut size as *mut _ as *mut c_void,
        );
        CFRelease(size_value);
        if size_ok == 0 {
            return Some(CGRect {
                origin: point,
                size: CGSize {
                    width: 1.0,
                    height: 1.0,
                },
            });
        }

        Some(CGRect {
            origin: point,
            size,
        })
    }
}

/// Clamps the window's top-left corner `(x, y)` (same coordinate space as area, physical px)
/// into the given rect, **guaranteeing the whole window (incl. its w×h) stays visible inside
/// the area**. With a work area this avoids covering the taskbar.
///
/// Pure function, no Win32 dependencies, so multi-monitor / negative-origin / weird-DPI
/// inputs are unit-testable. issue #470: the Windows branch previously clamped only the top
/// edge (`y.max(mon.top)`), leaving left/right/bottom unclamped — with negative multi-monitor
/// coordinates the capsule could land off-screen with no observability. All four edges are
/// clamped now.
///
/// When area is smaller than the window (`area_right - w < area_left`), `max_x` degenerates
/// to `area_left` and `clamp` pulls the top-left corner back to the area's top-left, keeping
/// at least the top-left corner visible without overflowing into negative out-of-bounds.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn clamp_to_monitor(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    area_left: i32,
    area_top: i32,
    area_right: i32,
    area_bottom: i32,
) -> (i32, i32) {
    // Right/bottom bounds = area's bottom-right minus the window's own size, so the whole
    // window stays visible. saturating_sub guards against subtraction overflow when
    // area_right/area_bottom are extremely small (near i32::MIN).
    let max_x = area_right.saturating_sub(w).max(area_left);
    let max_y = area_bottom.saturating_sub(h).max(area_top);
    let clamped_x = x.clamp(area_left, max_x);
    let clamped_y = y.clamp(area_top, max_y);
    (clamped_x, clamped_y)
}

/// Places the QA window at the bottom-center of the screen just above the capsule. Called
/// once during Tauri startup and before each show, so positions stay correct after the user
/// switches monitors.
fn position_qa_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    let Some((frame, scale)) = chat_window_work_area(window, true)? else {
        return Ok(());
    };
    let capsule_height = capsule_height_for_qa();
    let (x, y) = bottom_center_position(
        frame,
        QA_WINDOW_WIDTH,
        QA_WINDOW_EXPANDED_HEIGHT,
        capsule_height + QA_WINDOW_GAP_TO_CAPSULE,
    );
    window.set_position(tauri::PhysicalPosition::new(
        (x * scale).round() as i32,
        (y * scale).round() as i32,
    ))?;
    window.set_size(tauri::PhysicalSize::new(
        (QA_WINDOW_WIDTH * scale).round() as u32,
        (QA_WINDOW_HEIGHT * scale).round() as u32,
    ))?;
    Ok(())
}

/// Called on the native main thread by the restricted QA command. The compact
/// state has a genuinely small native frame, so hidden content cannot eat clicks.
pub(crate) fn set_qa_window_expanded<R: tauri::Runtime>(
    app: &AppHandle<R>,
    expanded: bool,
) -> Result<(), String> {
    let window = app
        .get_webview_window("qa")
        .ok_or_else(|| "qa_window_unavailable".to_string())?;
    let area = chat_window_work_area(&window, false).map_err(|_| "qa_geometry_unavailable")?;
    let scale = area.map(|(_, scale)| scale).unwrap_or(
        window
            .scale_factor()
            .map_err(|_| "qa_geometry_unavailable")?,
    );
    let position = window
        .inner_position()
        .map_err(|_| "qa_geometry_unavailable")?
        .to_logical::<f64>(scale);
    let mut width = QA_WINDOW_WIDTH;
    let mut height = if expanded {
        QA_WINDOW_EXPANDED_HEIGHT
    } else {
        QA_WINDOW_HEIGHT
    };
    let (mut x, mut y) = (position.x, position.y);
    if let Some((frame, _)) = area {
        width = width.min((frame.width - 32.0).max(240.0));
        height = height.min((frame.height - 64.0).max(QA_WINDOW_HEIGHT));
        x = x.clamp(
            frame.x + 16.0,
            (frame.x + frame.width - width - 16.0).max(frame.x + 16.0),
        );
        y = y.clamp(
            frame.y + 32.0,
            (frame.y + frame.height - height - 16.0).max(frame.y + 32.0),
        );
    }
    window
        .set_position(tauri::PhysicalPosition::new(
            (x * scale).round() as i32,
            (y * scale).round() as i32,
        ))
        .map_err(|_| "qa_position_failed")?;
    window
        .set_size(tauri::PhysicalSize::new(
            (width * scale).round() as u32,
            (height * scale).round() as u32,
        ))
        .map_err(|_| "qa_resize_failed")?;
    Ok(())
}

/// Shows the QA window and emits a state event (the frontend subscribes to `qa:state`).
/// `content_kind` is an opaque string ("loading" / "answer" / "idle", etc.); the frontend
/// React view decides which to render. **Never** steals focus from the frontmost app
/// (keeps the Cmd+C fallback able to read the selection from the original app).
pub(crate) fn show_qa_window<R: tauri::Runtime>(app: &AppHandle<R>, content_kind: &str) {
    #[cfg(target_os = "android")]
    {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::open_qa_host(env, context)
        }) {
            Ok(()) => log::info!("[qa] android requested WarmupActivity foreground for QA"),
            Err(error) => log::warn!("[qa] android failed to foreground WarmupActivity: {error}"),
        }
        log::info!("[qa] android publish qa:state to main kind={content_kind}");
        tauri_events::publish(
            app,
            None,
            openless_core::BackendEventKind::QaState(openless_core::QaStateEvent::simple(
                openless_core::QaStateKind::Idle,
            )),
        );
        return;
    }

    let Some(window) = ensure_qa_window(app) else {
        log::info!("[qa] show 跳过：qa 窗口不存在 (content_kind={content_kind})");
        return;
    };
    // Center only on first show; afterwards keep the user-dragged position.
    if !QA_WINDOW_POSITIONED.load(Ordering::Relaxed) {
        if let Err(e) = position_qa_window(&window) {
            log::warn!("[qa] position before first show failed: {e}");
        }
        QA_WINDOW_POSITIONED.store(true, Ordering::Relaxed);
    }
    // macOS: don't use window.show() (it does makeKeyAndOrderFront, pushing OpenLess to
    // frontmost; the subsequent capture_selection AX read / Cmd+C fallback would then run
    // against OpenLess's own webview → can't grab the original app's selection). Use
    // orderFrontRegardless so the window is visible but **not** key; the user's app stays
    // frontmost and AX can still read the selection. Standard Spotlight / Raycast approach.
    //
    // ⚠️ Critical: every NSWindow operation must run on the main thread; macOS 26 asserts
    // this hard (violations SIGTRAP immediately). show_qa_window is often called from a
    // tokio worker (qa_hotkey_bridge_loop), so raw ObjC msg_send must be dispatched via
    // `app.run_on_main_thread`. See issue #118 v2.
    #[cfg(target_os = "macos")]
    {
        let window_clone = window.clone();
        let _ = app.run_on_main_thread(move || {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            match window_clone.ns_window() {
                Ok(handle) => {
                    let ns = handle as *mut AnyObject;
                    if ns.is_null() {
                        log::warn!("[qa] ns_window null; falling back to window.show()");
                        let _ = window_clone.show();
                    } else {
                        unsafe {
                            let _: () = msg_send![ns, orderFrontRegardless];
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[qa] ns_window unavailable: {e}; falling back to window.show()");
                    let _ = window_clone.show();
                }
            }
        });
    }
    #[cfg(target_os = "windows")]
    if !show_qa_window_no_activate(&window) {
        log::warn!("[qa] show_no_activate failed; falling back to window.show()");
        if let Err(e) = window.show() {
            log::warn!("[qa] show fallback failed: {e}");
        }
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    if let Err(e) = window.show() {
        log::warn!("[qa] show failed: {e}");
    }
    // Cancel the pending exit-hide (fast close-open) and replay the entrance animation.
    QA_PANEL_EPOCH.fetch_add(1, Ordering::SeqCst);
    let _ = app.emit_to("qa", "chat-panel:shown", serde_json::json!({}));
    tauri_events::publish(
        app,
        None,
        openless_core::BackendEventKind::QaState(openless_core::QaStateEvent::simple(
            openless_core::QaStateKind::Idle,
        )),
    );
}

/// Converts the chat-panel floating windows (qa / less-computer) into non-activating
/// NSPanels (macOS; same technique as the capsule).
///
/// Goal: the panel can become key window **without OpenLess being activated** (Spotlight
/// style). Previously typing focus went through `window.set_focus()`, which activates the
/// whole app — the main window (settings page) comes to the foreground too, frontmost
/// becomes OpenLess itself, and AX can no longer read the original app's selection. After
/// the NonactivatingPanel conversion, clicking the input box / makeKeyWindow gives only the
/// panel keyboard focus: the app stays in the background, the main window doesn't move, and
/// frontmost is always the user's original app.
/// Also CanJoinAllSpaces + FullScreenAuxiliary: selection polish often happens in fullscreen
/// apps, so the panel must be able to overlay them.
#[cfg(target_os = "macos")]
fn make_chat_window_panel_macos<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, tag: &str) {
    use tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior;
    use tauri_nspanel::WebviewWindowExt;
    match window.to_panel() {
        Ok(panel) => {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            let raw = &*panel as *const _ as *mut AnyObject;
            if !raw.is_null() {
                unsafe {
                    let current: usize = msg_send![raw, styleMask];
                    let _: () = msg_send![raw, setStyleMask: current | (1usize << 7)];
                }
            }
            // Floating level (NSFloatingWindowLevel): above normal windows, below the
            // menu bar / capsule (25).
            panel.set_level(3);
            panel.set_collection_behaviour(
                NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces,
            );
            log::info!("[{tag}] converted to nonactivating NSPanel");
        }
        Err(e) => log::warn!("[{tag}] to_panel failed: {e:?}"),
    }
}

/// Drag fix for the chat-panel floating windows (qa / less-computer) on macOS.
///
/// `focus: false` makes Tauri create the window as a nonactivating panel (so it doesn't
/// steal focus from the frontmost app). The cost: AppKit's `performWindowDragWithEvent:`
/// doesn't work on nonactivating windows, so neither `data-tauri-drag-region` nor
/// `WebviewWindow::start_dragging()` can drag them.
///
/// Fix: enable the NSWindow's `movableByWindowBackground` — this path doesn't depend on
/// the window being key, same technique as the Spotlight / Raycast floaters. Set once and
/// it holds for the window's lifetime.
#[cfg(target_os = "macos")]
fn make_chat_window_draggable_macos<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    tag: &str,
) {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};
    let Ok(handle) = window.ns_window() else {
        log::warn!("[{tag}] ns_window unavailable; drag fix skipped");
        return;
    };
    let ns_window = handle as *mut AnyObject;
    if ns_window.is_null() {
        log::warn!("[{tag}] ns_window null; drag fix skipped");
        return;
    }
    // Exception guard: AppKit raises an NSException from inside setMovable when drag
    // margins are inconsistent. Uncaught, the exception crosses the Rust boundary and
    // aborts the whole app. Catch it and degrade to a log (dragging breaks but the app
    // lives); the log names the specific exception to ease root-causing.
    // SAFETY: the closure contains only two void-returning ObjC message sends and no Rust
    // values needing destructors; exception unwinding past the closure frame stays
    // memory-safe.
    let result = unsafe {
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let _: () = msg_send![ns_window, setMovableByWindowBackground: Bool::YES];
            let _: () = msg_send![ns_window, setMovable: Bool::YES];
        }))
    };
    match result {
        Ok(()) => log::info!("[{tag}] NSWindow movableByWindowBackground=YES"),
        Err(e) => {
            log::error!("[{tag}] drag setup raised ObjC exception (caught, drag disabled): {e:?}")
        }
    }
}

/// Lazily creates the QA window: it used to be created eagerly in tauri.conf.json (keeping
/// a resident WebKit process); now it is built on first show — nonexistent while idle →
/// saves one resident webview. Config matches the old tauri.conf qa block item by item
/// ("center": false ⇒ **don't** call .center(); "focus": false ⇒ focused(false)).
/// Key: make_qa_window_draggable_macos used to run once at startup; it must be re-applied
/// at creation time here, otherwise the lazily created QA window can't be dragged on macOS.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn ensure_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<tauri::WebviewWindow<R>> {
    if let Some(w) = app.get_webview_window("qa") {
        return Some(w);
    }
    let built =
        WebviewWindowBuilder::new(app, "qa", WebviewUrl::App("index.html?window=qa".into()))
            .title("OpenLess QA")
            .inner_size(QA_WINDOW_WIDTH, QA_WINDOW_HEIGHT)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .visible(false)
            .accept_first_mouse(true)
            .build();
    match built {
        Ok(w) => {
            // ⚠️ NSWindow operations must run on the main thread (macOS 26 hard
            // requirement). ensure_qa_window is often entered from a tokio worker (hotkey
            // bridge loop); calling setMovable directly on the worker thread makes AppKit
            // throw an NSException from _postWindowNeedsToResetDragMargins and crash the
            // whole app (root cause of four same-stack crashes on 2026-07-03) — dispatch
            // back to the main thread.
            #[cfg(target_os = "macos")]
            {
                let w_clone = w.clone();
                let _ = app.run_on_main_thread(move || {
                    make_chat_window_panel_macos(&w_clone, "qa");
                    make_chat_window_draggable_macos(&w_clone, "qa");
                });
            }
            Some(w)
        }
        Err(e) => {
            log::warn!("[qa] lazy window create failed: {e}");
            None
        }
    }
}

// Mobile routes QA to the main window (show_qa_window returns early on Android); Android's
// WebviewWindowBuilder lacks the desktop methods, so this stub just returns the existing
// window (compiles, but unreachable at runtime).
#[cfg(any(target_os = "android", target_os = "ios"))]
fn ensure_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<tauri::WebviewWindow<R>> {
    app.get_webview_window("qa")
}

/// Lazily creates the Less Computer window. On macOS it is additionally converted into a
/// focus-stealing-free NSPanel.
#[cfg(target_os = "macos")]
fn ensure_less_computer_window<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    if let Some(w) = app.get_webview_window("less-computer") {
        return Some(w);
    }
    match WebviewWindowBuilder::new(
        app,
        "less-computer",
        WebviewUrl::App("index.html?window=less-computer".into()),
    )
    .title("OpenLess Less Computer")
    .inner_size(LESS_COMPUTER_WINDOW_WIDTH, LESS_COMPUTER_WINDOW_HEIGHT)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(true)
    .min_inner_size(760.0, 520.0)
    .focused(false)
    .visible(false)
    .accept_first_mouse(true)
    .build()
    {
        Ok(w) => {
            // Same as QA: convert to a non-activating NSPanel (typing doesn't activate the
            // app) + drag fix (movableByWindowBackground; must be on the main thread, see
            // make_chat_window_draggable_macos).
            let w_clone = w.clone();
            let _ = app.run_on_main_thread(move || {
                make_chat_window_panel_macos(&w_clone, "less-computer");
                make_chat_window_draggable_macos(&w_clone, "less-computer");
                LESS_COMPUTER_WINDOW_POSITIONED.store(false, Ordering::Relaxed);
            });
            Some(w)
        }
        Err(e) => {
            log::warn!("[less-computer] lazy window create failed: {e}");
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn ensure_less_computer_window<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window("less-computer") {
        return Some(window);
    }
    WebviewWindowBuilder::new(
        app,
        "less-computer",
        WebviewUrl::App("index.html?window=less-computer".into()),
    )
    .title("OpenLess Less Computer")
    .inner_size(LESS_COMPUTER_WINDOW_WIDTH, LESS_COMPUTER_WINDOW_HEIGHT)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(true)
    .min_inner_size(760.0, 520.0)
    .focused(false)
    .visible(false)
    .build()
    .map(Some)
    .unwrap_or_else(|error| {
        log::warn!("[less-computer] lazy window create failed: {error}");
        None
    })
}

/// Hides a chat panel with an exit animation: emits `chat-panel:closing` first so the
/// frontend plays the exit animation, then truly hides after 240ms. A show in between
/// (epoch advances) cancels the pending hide — avoiding "the close animation is still
/// playing, the user re-summons, and an old timer hides the window".
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn hide_chat_window_animated<R: tauri::Runtime>(
    app: &AppHandle<R>,
    label: &str,
    epoch: &'static std::sync::atomic::AtomicU64,
) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let token = epoch.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit_to(label, "chat-panel:closing", serde_json::json!({}));
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(240)).await;
        if epoch.load(Ordering::SeqCst) == token {
            let _ = window.hide();
        }
    });
}

/// Hides the QA window. Shared by commands::qa_window_dismiss and coordinator session teardown.
pub(crate) fn hide_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "android")]
    {
        let _ = app.emit_to("main", "qa:dismiss", serde_json::json!({}));
        return;
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    hide_chat_window_animated(app, "qa", &QA_PANEL_EPOCH);
}

/// 「润色结果」不再有独立窗口：复用选区助手（qa）面板切到润色模式。核心在
/// 「预览确认」输出模式下照旧只发 `HostAction::ShowSelectionPreview`。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn show_selection_polish_preview<R: tauri::Runtime>(app: &AppHandle<R>) {
    SELECTION_POLISH_PREVIEW_PENDING.store(true, Ordering::SeqCst);
    show_qa_window(app, "polish-preview");
    // 面板可能是本次才懒创建（刚挂载，订阅还没装上）——它在挂载时也会自己
    // 拉一次负载（见 `get_selection_polish_preview` 的 pending 门禁）。
    let _ = app.emit_to("qa", SELECTION_POLISH_PREVIEW_SHOWN, ());
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn show_selection_polish_preview<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 退出「润色结果」模式。是否顺带关掉面板交给前端：面板可能正处在提问对话中，
/// 不能被润色流程的收尾动作一起关掉（与 egui 侧 `HideSelectionPreview` 一致）。
pub(crate) fn hide_selection_polish_preview<R: tauri::Runtime>(app: &AppHandle<R>) {
    clear_selection_polish_preview_pending();
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = app;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let _ = app.emit_to("qa", SELECTION_POLISH_PREVIEW_HIDE, ());
}

/// Selection voice: after speaking, the user chooses to ask a question or edit.
#[cfg(all(not(mobile), target_os = "windows"))]
fn ensure_selection_voice_intent_prompt_window<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window("selection-voice-intent") {
        return Some(window);
    }
    WebviewWindowBuilder::new(
        app,
        "selection-voice-intent",
        WebviewUrl::App("index.html?window=selection-voice-intent".into()),
    )
    .title("OpenLess 選區語音")
    .inner_size(420.0, 280.0)
    .min_inner_size(360.0, 240.0)
    .resizable(true)
    .always_on_top(true)
    .visible(false)
    .build()
    .map(Some)
    .unwrap_or_else(|error| {
        log::warn!("[selection-voice] create intent prompt window failed: {error}");
        None
    })
}

#[cfg(all(not(mobile), target_os = "windows"))]
pub(crate) fn show_selection_voice_intent_prompt<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(window) = ensure_selection_voice_intent_prompt_window(app) else {
        return;
    };
    if let Err(error) = window.show() {
        log::warn!("[selection-voice] show intent prompt failed: {error}");
        return;
    }
    if let Err(error) = window.set_focus() {
        log::warn!("[selection-voice] focus intent prompt failed: {error}");
    }
    let _ = app.emit_to("selection-voice-intent", "selection-voice-intent:shown", ());
}

#[cfg(not(all(not(mobile), target_os = "windows")))]
pub(crate) fn show_selection_voice_intent_prompt<R: tauri::Runtime>(_app: &AppHandle<R>) {}

#[cfg(all(not(mobile), target_os = "windows"))]
pub(crate) fn hide_selection_voice_intent_prompt<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("selection-voice-intent") {
        let _ = window.hide();
    }
}

#[cfg(not(all(not(mobile), target_os = "windows")))]
pub(crate) fn hide_selection_voice_intent_prompt<R: tauri::Runtime>(_app: &AppHandle<R>) {}

// ───────────────────────── Less Computer window ─────────────────────────
//
// Chat window for the Less Computer voice Agent (window label = "less-computer").
// Windows/macOS both provide the entry point, hotkey, and chat window, sharing Core
// sessions and events; native window manipulation stays per-platform — macOS
// NSWindow/AppKit adjustments must not leak into the Windows build.
// On Linux the product window is owned by the egui Host, not created in Tauri;
// unsupported platforms keep a no-op.

/// Less Computer defaults to a desktop workspace; users can resize it afterwards.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const LESS_COMPUTER_WINDOW_WIDTH: f64 = 990.0;
#[cfg(any(target_os = "macos", target_os = "windows"))]
const LESS_COMPUTER_WINDOW_HEIGHT: f64 = 680.0;

/// Position the initial workspace within the monitor; subsequent shows preserve user geometry.
#[cfg(target_os = "macos")]
fn position_less_computer_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<()> {
    let Some((frame, scale)) = chat_window_work_area(window, true)? else {
        return Ok(());
    };
    let width = LESS_COMPUTER_WINDOW_WIDTH.min((frame.width - 32.0).max(760.0));
    let height = LESS_COMPUTER_WINDOW_HEIGHT.min((frame.height - 32.0).max(520.0));
    let x = frame.x + (frame.width - width).max(0.0) / 2.0;
    let y = frame.y + (frame.height - height).max(0.0) / 2.0;
    window.set_position(tauri::PhysicalPosition::new(
        (x * scale).round() as i32,
        (y * scale).round() as i32,
    ))?;
    window.set_size(tauri::PhysicalSize::new(
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    ))?;
    Ok(())
}

/// Shows the Less Computer window (no focus steal from the frontmost app, same technique
/// as QA). `macos` build only.
#[cfg(target_os = "macos")]
pub(crate) fn show_less_computer_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(window) = ensure_less_computer_window(app) else {
        log::info!("[less-computer] show 跳过：窗口不存在");
        return;
    };
    let window_clone = window.clone();
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        // This helper is also called from the Tokio worker that executes a text or
        // voice Agent turn. Keep every AppKit-backed window mutation on the main
        // thread; macOS aborts the process if a converted NSPanel is resized or moved
        // from that worker while WebKit is servicing its custom URL scheme.
        if !LESS_COMPUTER_WINDOW_POSITIONED.load(Ordering::Relaxed) {
            match position_less_computer_window(&window_clone) {
                Ok(()) => LESS_COMPUTER_WINDOW_POSITIONED.store(true, Ordering::Relaxed),
                Err(e) => log::warn!("[less-computer] position before show failed: {e}"),
            }
        }
        // A lazily-created window starts with Tauri's visible=false state. Cocoa's
        // orderFrontRegardless alone does not always clear that state, leaving the first
        // text-only launch invisible even though the NSPanel was created successfully.
        if let Err(e) = window_clone.show() {
            log::warn!("[less-computer] window.show before orderFront failed: {e}");
        }
        match window_clone.ns_window() {
            Ok(handle) => {
                let ns = handle as *mut AnyObject;
                if ns.is_null() {
                    log::warn!("[less-computer] ns_window null; falling back to window.show()");
                    let _ = window_clone.show();
                } else {
                    unsafe {
                        let _: () = msg_send![ns, orderFrontRegardless];
                    }
                }
            }
            Err(e) => {
                log::warn!("[less-computer] ns_window unavailable: {e}; falling back to show()");
                let _ = window_clone.show();
            }
        }
    });
    // Cancel the pending exit-hide (fast close-open) and replay the entrance animation.
    LESS_COMPUTER_PANEL_EPOCH.fetch_add(1, Ordering::SeqCst);
    let _ = app.emit_to("less-computer", "chat-panel:shown", serde_json::json!({}));
}

#[cfg(target_os = "windows")]
pub(crate) fn show_less_computer_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(window) = ensure_less_computer_window(app) else {
        return;
    };
    if let Err(error) = window.show() {
        log::warn!("[less-computer] show failed: {error}");
        return;
    }
    LESS_COMPUTER_PANEL_EPOCH.fetch_add(1, Ordering::SeqCst);
    let _ = app.emit_to("less-computer", "chat-panel:shown", serde_json::json!({}));
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn show_less_computer_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// Hides the Less Computer window. Shared by the dismiss command / session teardown.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn hide_less_computer_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    hide_chat_window_animated(app, "less-computer", &LESS_COMPUTER_PANEL_EPOCH);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn hide_less_computer_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// Less Computer presents work and recording feedback inside its panel.
/// Keep the host port callable without creating a full-screen glow WebView.
pub(crate) fn show_less_computer_glow<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// Hides the fullscreen rainbow-outline overlay.
#[cfg(target_os = "macos")]
pub(crate) fn hide_less_computer_glow<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("less-computer-glow") {
        // issue #470: notify the frontend "hidden" first so it unloads the fullscreen glow
        // layer (4 infinite animations) — once the webview hides, GPU cost is zero;
        // otherwise the webview keeps compositing the glow layer after .hide()
        // (Windows especially never releases the animations).
        let _ = window.emit("less-computer-glow:active", false);
        let _ = window.hide();
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_less_computer_glow<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// Hands focus back to the QA window after the selection is captured (second half of the
/// Windows focus-dance). Called by begin_qa_session when capture_selection finishes;
/// no-op on non-Windows. issue #466.
#[cfg(target_os = "windows")]
pub(crate) fn refocus_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("qa") {
        let _ = show_qa_window_no_activate(&window);
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn refocus_qa_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

#[cfg(target_os = "windows")]
fn show_qa_window_no_activate<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    // The name is historical; the actual behavior is now "show + focus" — the QA webview
    // must really get keyboard focus so ESC reaches the React listener and the X button's
    // first click isn't swallowed by the OS as an activation click.
    //
    // Why Tauri's show() / set_focus() instead of Win32 SetForegroundWindow + SetFocus:
    //   - SetFocus(host_hwnd) alone doesn't guarantee the WebView2 child receives keyboard
    //     events; WebView2 child windows have their own focus model. Tauri's internal
    //     webview-specific path actually delivers focus to the webview.
    //   - SetForegroundWindow can be rejected under Win11 focus-stealing prevention.
    //     Tauri 2.x has fallbacks in its cross-platform abstraction (temporary SPI
    //     adjustment / input queue attach).
    //
    // Trade-off vs issue #164 "QA window must not steal the frontmost app's focus": the
    // window briefly becomes foreground when it appears, but begin_qa_session's focus-dance
    // temporarily returns focus to the user's original app before capturing the selection
    // (see the same-issue comment in coordinator.rs), and refocus_qa_window takes it back
    // afterwards — the selection path still works; issue #164 being briefly violated for
    // "the frame the QA window appears" is the cost of the #466 fix.
    if window.show().is_err() {
        return false;
    }
    let _ = window.set_focus();
    true
}

/// Physical rect (virtual desktop coordinates) of the input target monitor + DPI scale.
#[cfg(target_os = "windows")]
pub(crate) struct ForegroundMonitor {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
    /// Work-area rect (physical px, taskbar excluded). Consistent across platforms: the
    /// capsule clamps into the work area first, avoiding the taskbar. Falls back to the
    /// full-screen rect when unavailable. issue #470.
    pub(crate) work_left: i32,
    pub(crate) work_top: i32,
    pub(crate) work_right: i32,
    pub(crate) work_bottom: i32,
    /// The monitor's effective DPI scale (1.0 = 96dpi).
    pub(crate) scale: f64,
}

/// Locates, via Win32, the monitor of "the current foreground window (= the app the user
/// is typing in)". With multiple monitors, places the capsule on "the screen being typed
/// on". `window.current_monitor()` returns the monitor the capsule window itself is on, so
/// it cannot be used to follow the input position.
#[cfg(target_os = "windows")]
pub(crate) fn foreground_window_monitor() -> Option<ForegroundMonitor> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        let hwnd = GetForegroundWindow();
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if hmon.is_invalid() {
            return None;
        }
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
            return None;
        }
        let mut dpi_x: u32 = 96;
        let mut dpi_y: u32 = 96;
        // Fall back to 96dpi when unreadable; positioning must not fail as a whole.
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        Some(ForegroundMonitor {
            left: mi.rcMonitor.left,
            top: mi.rcMonitor.top,
            right: mi.rcMonitor.right,
            bottom: mi.rcMonitor.bottom,
            work_left: mi.rcWork.left,
            work_top: mi.rcWork.top,
            work_right: mi.rcWork.right,
            work_bottom: mi.rcWork.bottom,
            scale: (dpi_x as f64 / 96.0).max(0.1),
        })
    }
}

/// Init and card-return paths use the default stage; the recording display path passes the
/// current style explicitly.
pub(crate) fn position_capsule_bottom_center<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    translation_active: bool,
) -> tauri::Result<()> {
    position_capsule_bottom_center_with_style(window, translation_active, types::CapsuleStyle::Siri)
}

/// Uses the system-reported work area, automatically avoiding the Dock / taskbar and
/// following auto-hide settings.
pub(crate) fn position_capsule_bottom_center_with_style<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    _translation_active: bool,
    style: types::CapsuleStyle,
) -> tauri::Result<()> {
    let bounds = capsule_window_bounds_for_style(style);
    const EDGE_GAP: f64 = 12.0;
    // Typeless hugs the work-area bottom edge (the work area already excludes Dock / taskbar).
    let bottom_gap = match style {
        types::CapsuleStyle::Typeless => 0.0,
        _ => EDGE_GAP,
    };

    // Windows: follow the monitor of "the app being typed in", so the capsule doesn't
    // always land on the primary screen / the capsule's own screen on multi-monitor setups.
    #[cfg(target_os = "windows")]
    {
        if let Some(mon) = foreground_window_monitor() {
            let scale = mon.scale;
            let phys_w = (bounds.width * scale).round() as i32;
            let phys_h = (bounds.height * scale).round() as i32;
            window.set_size(PhysicalSize::new(
                phys_w.max(1) as u32,
                phys_h.max(1) as u32,
            ))?;

            let (work_l, work_t, work_r, work_b) =
                if mon.work_right > mon.work_left && mon.work_bottom > mon.work_top {
                    (mon.work_left, mon.work_top, mon.work_right, mon.work_bottom)
                } else {
                    (mon.left, mon.top, mon.right, mon.bottom)
                };
            let x = work_l + ((work_r - work_l - phys_w) / 2).max(0);
            let y = work_b - phys_h - (bottom_gap * scale).round() as i32;
            let (clamped_x, clamped_y) =
                clamp_to_monitor(x, y, phys_w, phys_h, work_l, work_t, work_r, work_b);
            log::debug!(
                "[capsule] win position: mon=({},{})..({},{}) work=({},{})..({},{}) scale={:.2} size=({}x{}) -> raw=({},{}) clamped=({},{})",
                mon.left, mon.top, mon.right, mon.bottom,
                work_l, work_t, work_r, work_b,
                scale, phys_w, phys_h, x, y, clamped_x, clamped_y
            );
            window.set_position(PhysicalPosition::new(clamped_x, clamped_y))?;
            return Ok(());
        }
        // Only fall back to the current_monitor logic below when Win32 can't find the
        // foreground monitor.
    }

    // macOS: follow the monitor under the mouse cursor, not the monitor the capsule window
    // last sat on — on any external screen / any Space, the hidden capsule can move before
    // appearing.
    #[cfg(target_os = "macos")]
    {
        if let Some(mon) = capsule_target_monitor(window) {
            window.set_size(LogicalSize::new(bounds.width, bounds.height))?;
            let (x, y) = bottom_center_position(
                mon.logical_work_area(),
                bounds.width,
                bounds.height,
                bottom_gap,
            );
            log::debug!(
                "[capsule] mac position: mon=({},{}) size=({}x{}) scale={:.2} -> logical=({:.1},{:.1})",
                mon.physical_x,
                mon.physical_y,
                mon.physical_width,
                mon.physical_height,
                mon.scale,
                x,
                y
            );
            window.set_position(LogicalPosition::new(x, y))?;
            return Ok(());
        }
    }

    let monitor = match window.current_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    window.set_size(LogicalSize::new(bounds.width, bounds.height))?;

    let scale = monitor.scale_factor();
    let size = &monitor.work_area().size;
    let pos = &monitor.work_area().position;
    let frame = logical_monitor_frame(pos.x, pos.y, size.width, size.height, scale);
    let (x, y) = bottom_center_position(frame, bounds.width, bounds.height, bottom_gap);
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CapsuleWindowBounds {
    width: f64,
    height: f64,
    bottom_inset: f64,
}

fn capsule_window_bounds(translation_active: bool) -> CapsuleWindowBounds {
    // Pure light-effect voice stage (siri-glsl original proportions): light bar spans
    // ~420px + glow diffusion margin. Kept in sync with the frontend's VOICE_ORB_STAGE_*
    // in src/lib/capsuleLayout.ts.
    let _ = translation_active;
    capsule_window_bounds_for_style(types::CapsuleStyle::Siri)
}

fn capsule_window_bounds_for_style(style: types::CapsuleStyle) -> CapsuleWindowBounds {
    CapsuleWindowBounds {
        // The typeless window area is 1/5 of the original size (460×128); the frontend
        // scales content in sync with CSS zoom — see CapsuleStyles.css and
        // src/lib/capsuleLayout.ts.
        width: match style {
            types::CapsuleStyle::Typeless => 206.0,
            types::CapsuleStyle::Siri | types::CapsuleStyle::Classic => 460.0,
        },
        height: match style {
            types::CapsuleStyle::Siri => 180.0,
            types::CapsuleStyle::Classic => 100.0,
            types::CapsuleStyle::Typeless => 57.0,
        },
        bottom_inset: 0.0,
    }
}

fn capsule_visual_height(_translation_active: bool) -> f64 {
    // The QA panel reuses this visual height for vertical spacing; the capsule itself is
    // positioned by style size and the system work area.
    140.0
}

fn capsule_height_for_qa() -> f64 {
    capsule_visual_height(false)
}

#[cfg(test)]
mod tests {
    use super::{
        bottom_center_position, capsule_height_for_qa, capsule_visual_height,
        capsule_window_bounds, capsule_window_bounds_for_style, clamp_to_monitor,
        frame_contains_point, frame_distance_to_point_squared, logical_monitor_frame,
        parse_tray_style_pack_menu_id, resolve_tray_style_pack_id, rotate_log_if_too_large,
        tray_style_menu_enabled, tray_style_pack_menu_entries, tray_style_pack_menu_id,
        LogicalMonitorFrame, TrayLabels, LOG_ROTATE_LIMIT_BYTES,
    };
    #[cfg(target_os = "macos")]
    use super::{pick_monitor_for_anchor_point, CapsuleTargetMonitor};
    use crate::types::{
        builtin_style_pack_for_mode, CapsuleStyle, PolishMode, StylePack, StylePackKind,
    };
    use std::io::Write;

    #[test]
    fn tray_style_menu_is_windows_only() {
        #[cfg(target_os = "windows")]
        assert!(tray_style_menu_enabled());

        #[cfg(not(target_os = "windows"))]
        assert!(!tray_style_menu_enabled());
    }

    #[test]
    fn tray_style_menu_lists_enabled_packs_and_marks_active_id() {
        let imported = StylePack {
            id: "imported.meeting".into(),
            name: "会议纪要".into(),
            kind: StylePackKind::Imported,
            base_mode: PolishMode::Structured,
            ..StylePack::default()
        };
        let duplicate_base_mode = StylePack {
            id: "imported.structured".into(),
            name: "自定义结构化".into(),
            kind: StylePackKind::Imported,
            base_mode: PolishMode::Structured,
            ..StylePack::default()
        };
        let disabled = StylePack {
            id: "imported.disabled".into(),
            name: "已禁用".into(),
            kind: StylePackKind::Imported,
            base_mode: PolishMode::Structured,
            enabled: false,
            ..StylePack::default()
        };

        let packs = vec![
            builtin_style_pack_for_mode(PolishMode::Raw),
            builtin_style_pack_for_mode(PolishMode::Light),
            builtin_style_pack_for_mode(PolishMode::Structured),
            builtin_style_pack_for_mode(PolishMode::Formal),
            imported,
            duplicate_base_mode,
            disabled,
        ];
        let entries = tray_style_pack_menu_entries(
            &packs,
            "imported.meeting",
            TrayLabels::for_locale("zh-CN"),
        );

        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.pack_id.as_str(), entry.label.as_str(), entry.checked))
                .collect::<Vec<_>>(),
            vec![
                ("builtin.raw", "原文", false),
                ("builtin.light", "轻度润色", false),
                ("builtin.structured", "清晰结构", false),
                ("builtin.formal", "正式表达", false),
                ("imported.meeting", "会议纪要", true),
                ("imported.structured", "自定义结构化", false),
            ]
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.checked)
                .map(|entry| entry.pack_id.as_str())
                .collect::<Vec<_>>(),
            vec!["imported.meeting"]
        );
        assert_eq!(entries[0].id, tray_style_pack_menu_id("builtin.raw"));
    }

    #[test]
    fn tray_labels_follow_locale_and_localize_builtin_styles() {
        let english = TrayLabels::for_locale("en");
        assert_eq!(english.toggle, "Show main window");
        assert_eq!(english.style, "Output style");
        assert_eq!(english.microphone, "Select microphone");
        assert_eq!(english.default_microphone, "System default microphone");
        assert_eq!(english.quit, "Quit OpenLess");
        assert_eq!(english.style_pack_name(PolishMode::Light), "Light polish");
        assert_eq!(
            english.default_device_label("USB microphone"),
            "USB microphone (System default)"
        );

        let chinese = TrayLabels::for_locale("zh-CN");
        assert_eq!(chinese.style_pack_name(PolishMode::Structured), "清晰结构");
        assert_eq!(TrayLabels::for_locale("es").quit, "Salir de OpenLess");
        assert_eq!(
            TrayLabels::for_locale("fr").toggle,
            "Afficher la fenêtre principale"
        );
        assert_eq!(
            TrayLabels::for_locale("de").microphone,
            "Mikrofon auswählen"
        );
        assert_eq!(
            TrayLabels::for_locale("fr").style_pack_name(PolishMode::Structured),
            "Structuré"
        );
        assert_eq!(TrayLabels::for_locale("unknown").toggle, "显示主窗口");
    }

    #[test]
    fn tray_style_menu_localizes_builtins_and_preserves_custom_names() {
        let packs = vec![
            builtin_style_pack_for_mode(PolishMode::Raw),
            builtin_style_pack_for_mode(PolishMode::Light),
            StylePack {
                id: "imported.meeting".into(),
                name: "会议纪要".into(),
                kind: StylePackKind::Imported,
                base_mode: PolishMode::Structured,
                ..StylePack::default()
            },
        ];
        let entries = tray_style_pack_menu_entries(&packs, "", TrayLabels::for_locale("en"));

        assert_eq!(entries[0].label, "Raw");
        assert_eq!(entries[1].label, "Light polish");
        assert_eq!(entries[2].label, "会议纪要");
    }

    #[test]
    fn tray_style_menu_preserves_renamed_builtin_names() {
        let mut renamed = builtin_style_pack_for_mode(PolishMode::Raw);
        renamed.name = "My own raw style".into();
        let entries = tray_style_pack_menu_entries(&[renamed], "", TrayLabels::for_locale("en"));

        assert_eq!(entries[0].label, "My own raw style");
    }

    #[test]
    fn tray_style_menu_ids_are_stable_and_collision_safe() {
        let first = tray_style_pack_menu_id("imported.meeting");
        assert_eq!(first, "style-pack-id-imported.meeting");
        assert_eq!(first, tray_style_pack_menu_id("imported.meeting"));
        assert_ne!(first, tray_style_pack_menu_id("imported.structured"));
        assert_ne!(first, "style-structured");
    }

    #[test]
    fn tray_style_menu_id_parsing_rejects_malformed_and_stale_items() {
        let packs = vec![
            builtin_style_pack_for_mode(PolishMode::Raw),
            StylePack {
                id: "imported.disabled".into(),
                name: "已禁用".into(),
                kind: StylePackKind::Imported,
                base_mode: PolishMode::Raw,
                enabled: false,
                ..StylePack::default()
            },
        ];

        assert_eq!(
            parse_tray_style_pack_menu_id(&tray_style_pack_menu_id("builtin.raw")),
            Some("builtin.raw")
        );
        assert_eq!(
            resolve_tray_style_pack_id(&tray_style_pack_menu_id("builtin.raw"), &packs),
            Some("builtin.raw")
        );
        assert_eq!(
            resolve_tray_style_pack_id(&tray_style_pack_menu_id("imported.disabled"), &packs),
            None
        );
        assert_eq!(
            resolve_tray_style_pack_id(&tray_style_pack_menu_id("imported.deleted"), &packs),
            None
        );
        assert_eq!(parse_tray_style_pack_menu_id("style-pack-id-"), None);
        assert_eq!(parse_tray_style_pack_menu_id("style-raw"), None);
        assert_eq!(parse_tray_style_pack_menu_id("toggle"), None);
        assert_eq!(parse_tray_style_pack_menu_id("mic-default"), None);
    }

    #[test]
    fn capsule_window_bounds_match_voice_orb_stage() {
        let bounds = capsule_window_bounds(false);
        assert_eq!(
            (bounds.width, bounds.height, bounds.bottom_inset),
            (460.0, 180.0, 0.0)
        );
    }

    #[test]
    fn capsule_tracks_reserved_and_auto_hidden_system_bars() {
        let monitor = LogicalMonitorFrame {
            x: -1440.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        let work_area = LogicalMonitorFrame {
            height: 830.0,
            ..monitor
        };
        for style in [
            CapsuleStyle::Siri,
            CapsuleStyle::Classic,
            CapsuleStyle::Typeless,
        ] {
            let bounds = capsule_window_bounds_for_style(style);
            let (x, reserved_y) =
                bottom_center_position(work_area, bounds.width, bounds.height, 12.0);
            let (_, hidden_y) = bottom_center_position(monitor, bounds.width, bounds.height, 12.0);
            assert_eq!(x, work_area.x + (work_area.width - bounds.width) / 2.0);
            assert_eq!(reserved_y + bounds.height, 818.0);
            assert_eq!(hidden_y - reserved_y, 70.0);
        }
    }

    #[test]
    fn capsule_window_bounds_stay_fixed_for_translation_badge() {
        let bounds = capsule_window_bounds(true);
        assert_eq!(
            (bounds.width, bounds.height, bounds.bottom_inset),
            (460.0, 180.0, 0.0)
        );
    }

    #[test]
    fn typeless_capsule_window_is_one_fifth_of_the_old_area() {
        let bounds = capsule_window_bounds_for_style(CapsuleStyle::Typeless);
        assert_eq!((bounds.width, bounds.height), (206.0, 57.0));
    }

    #[test]
    fn capsule_visual_height_keeps_the_qa_anchor_reference() {
        assert_eq!(capsule_visual_height(true), 140.0);
    }

    #[test]
    fn qa_anchor_uses_normal_capsule_height_source() {
        assert_eq!(capsule_height_for_qa(), 140.0);
    }

    #[test]
    fn logical_monitor_frame_preserves_negative_origin() {
        let frame = logical_monitor_frame(-2560, 720, 5120, 2880, 2.0);

        assert_eq!(
            frame,
            LogicalMonitorFrame {
                x: -1280.0,
                y: 360.0,
                width: 2560.0,
                height: 1440.0,
            }
        );
    }

    #[test]
    fn monitor_frame_contains_points_with_negative_origins() {
        let frame = LogicalMonitorFrame {
            x: -1280.0,
            y: 360.0,
            width: 1280.0,
            height: 720.0,
        };

        assert!(frame_contains_point(frame, -640.0, 720.0));
        assert!(!frame_contains_point(frame, 10.0, 720.0));
        assert!(!frame_contains_point(frame, -640.0, 1080.0));
    }

    #[test]
    fn monitor_frame_distance_is_zero_inside_and_grows_outside() {
        let frame = LogicalMonitorFrame {
            x: 0.0,
            y: -900.0,
            width: 1440.0,
            height: 900.0,
        };

        assert_eq!(frame_distance_to_point_squared(frame, 100.0, -100.0), 0.0);
        assert_eq!(frame_distance_to_point_squared(frame, 100.0, 20.0), 400.0);
        assert_eq!(frame_distance_to_point_squared(frame, -10.0, -910.0), 200.0);
    }

    /// Typical dual-monitor layout: built-in Retina display left, external 1x display
    /// right. Physical coordinates are per-screen (each with its own scale), so the 1x
    /// external display starts at the built-in display's logical right edge x=1512.
    #[cfg(target_os = "macos")]
    fn built_in_and_external_monitors() -> [CapsuleTargetMonitor; 2] {
        [
            CapsuleTargetMonitor {
                physical_x: 0,
                physical_y: 0,
                physical_width: 3024,
                physical_height: 1964,
                work_x: 0,
                work_y: 48,
                work_width: 3024,
                work_height: 1760,
                scale: 2.0,
            },
            CapsuleTargetMonitor {
                physical_x: 1512,
                physical_y: 0,
                physical_width: 2560,
                physical_height: 1440,
                work_x: 1512,
                work_y: 24,
                work_width: 2560,
                work_height: 1320,
                scale: 1.0,
            },
        ]
    }

    /// Floating windows (QA / Less Computer panel / glow outline) follow the screen under
    /// the cursor, not the screen the window last sat on — if this ever regresses, floating
    /// windows get pinned to the primary display on multi-monitor setups.
    #[test]
    #[cfg(target_os = "macos")]
    fn anchor_point_selects_the_monitor_under_the_cursor() {
        let monitors = built_in_and_external_monitors();

        let on_external = pick_monitor_for_anchor_point(monitors, 2000.0, 600.0)
            .expect("external monitor should win when the cursor is on it");
        assert_eq!(on_external, monitors[1]);

        let on_built_in = pick_monitor_for_anchor_point(monitors, 700.0, 600.0)
            .expect("built-in monitor should win when the cursor is on it");
        assert_eq!(on_built_in, monitors[0]);
    }

    /// When the cursor briefly lands in the gap between screens (misaligned arrangement /
    /// freshly unplugged), fall back to the nearest screen instead of returning None and
    /// leaving the floating window in place.
    #[test]
    #[cfg(target_os = "macos")]
    fn anchor_point_outside_every_monitor_falls_back_to_the_nearest() {
        let monitors = built_in_and_external_monitors();

        // x=-100 is left of every screen; the built-in screen is nearest.
        let picked = pick_monitor_for_anchor_point(monitors, -100.0, 600.0)
            .expect("nearest monitor should be returned for an off-screen point");
        assert_eq!(picked, monitors[0]);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn anchor_point_without_any_monitor_yields_none() {
        assert!(pick_monitor_for_anchor_point(Vec::new(), 0.0, 0.0).is_none());
    }

    #[test]
    fn bottom_center_position_keeps_window_on_left_monitor() {
        let frame = LogicalMonitorFrame {
            x: -1440.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };

        let pos = bottom_center_position(frame, 380.0, 440.0, 184.0);

        assert_eq!(pos, (-910.0, 276.0));
    }

    // ---- #470: capsule four-edge clamp (pure function; synthetic multi-monitor /
    // negative-origin / 1.5x DPI inputs) ----

    #[test]
    fn clamp_to_monitor_leaves_on_screen_position_untouched() {
        // 1080p primary screen, slightly below center; the whole window is already
        // visible → returned unchanged.
        let (x, y) = clamp_to_monitor(800, 900, 264, 126, 0, 0, 1920, 1040);
        assert_eq!((x, y), (800, 900));
    }

    #[test]
    fn clamp_to_monitor_pulls_back_off_screen_right_and_bottom() {
        // x/y landed outside the screen's bottom-right → pulled back to "bottom-right minus
        // window size"; the whole window stays visible.
        let (x, y) = clamp_to_monitor(2000, 1200, 264, 126, 0, 0, 1920, 1040);
        assert_eq!((x, y), (1920 - 264, 1040 - 126));
        // The whole window's right/bottom edges land inside the area.
        assert!(x + 264 <= 1920);
        assert!(y + 126 <= 1040);
    }

    #[test]
    fn clamp_to_monitor_pushes_into_negative_origin_left_monitor() {
        // Secondary screen left of the primary (negative X origin); the point landed left
        // of it → clamped back to area_left.
        // At 1.5x DPI the window is oversized but the area is still wider than the window;
        // the top-left corner clamps to (-2560, top).
        let (x, y) = clamp_to_monitor(-3000, -100, 294, 138, -2560, 0, 0, 1440);
        assert_eq!(x, -2560);
        assert_eq!(y, 0);
        // Right/bottom stay inside the area.
        assert!(x >= -2560 && x + 294 <= 0);
        assert!(y >= 0 && y + 138 <= 1440);
    }

    #[test]
    fn clamp_to_monitor_respects_work_area_above_taskbar() {
        // Work-area bottom = 1040 (the taskbar occupies 1040..1080). The point lands in the
        // taskbar region (y=1030) and must clamp to above "work-area bottom - window height"
        // so the capsule never covers the taskbar.
        let (_x, y) = clamp_to_monitor(800, 1030, 264, 126, 0, 0, 1920, 1040);
        assert_eq!(y, 1040 - 126);
        assert!(y + 126 <= 1040);
    }

    #[test]
    fn clamp_to_monitor_degrades_gracefully_when_window_wider_than_area() {
        // Pathological input: area narrower than the window (rare, but must not panic or
        // overflow into negative out-of-bounds).
        // max_x clamps to area_left; clamp pulls the top-left corner back to area_left.
        let (x, y) = clamp_to_monitor(500, 500, 800, 600, 0, 0, 400, 300);
        assert_eq!((x, y), (0, 0));
    }

    #[test]
    fn oversized_log_rotates_to_single_archive() {
        let dir = std::env::temp_dir().join(format!("openless-log-rotate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("openless.log");
        let archive = dir.join("openless.log.1");

        {
            let mut file = std::fs::File::create(&log).unwrap();
            file.set_len(LOG_ROTATE_LIMIT_BYTES + 1).unwrap();
            file.write_all(b"x").unwrap();
        }
        std::fs::write(&archive, b"old").unwrap();

        rotate_log_if_too_large(&log).unwrap();

        assert!(!log.exists());
        assert!(archive.exists());
        assert!(std::fs::metadata(&archive).unwrap().len() > LOG_ROTATE_LIMIT_BYTES);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn small_log_does_not_rotate() {
        let dir = std::env::temp_dir().join(format!("openless-log-small-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("openless.log");
        let archive = dir.join("openless.log.1");
        std::fs::write(&log, b"small").unwrap();

        rotate_log_if_too_large(&log).unwrap();

        assert!(log.exists());
        assert!(!archive.exists());
        assert_eq!(std::fs::read(&log).unwrap(), b"small");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_log_does_not_rotate() {
        let dir = std::env::temp_dir().join(format!("openless-log-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("openless.log");
        let archive = dir.join("openless.log.1");

        rotate_log_if_too_large(&log).unwrap();

        assert!(!log.exists());
        assert!(!archive.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
