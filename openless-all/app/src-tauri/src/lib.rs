#![cfg_attr(
    target_os = "linux",
    allow(dead_code, unused_imports, unused_variables)
)]
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

mod android;
#[cfg(test)]
mod build_target;
mod asr;
mod audio_mute;
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
mod correction;
mod edit_plan;
mod selection_voice_intent;
// 托盘麦克风设备变更监听：macOS CoreAudio / Windows MMDevice 原生通知（空闲零唤醒），
// Linux 退化为纯轮询兜底。仅桌面端。详见 issue #470。
#[cfg(not(mobile))]
mod device_watch;
mod endpoint_security;
mod external_url;
#[cfg(not(mobile))]
mod global_hotkey_runtime;
// 读宿主 app 光标周围的正文，给 LLM 润色当上下文。唯一接触「别的应用的文档」的地方，
// 平台差异和安全硬拦全关在里面；目前仅 macOS 有实现，其余平台优雅降级。
mod host_document;
#[cfg(not(mobile))]
#[path = "hotkey.rs"]
mod hotkey;
#[cfg(mobile)]
#[path = "mobile_stubs/hotkey.rs"]
mod hotkey;
mod insertion;
#[cfg(target_os = "linux")]
mod linux_fcitx;
mod llm_gemini;
#[cfg(mobile)]
mod mobile_runtime;
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

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "linux")]
use gtk::prelude::WidgetExt;

const LOG_ROTATE_LIMIT_BYTES: u64 = 10 * 1024 * 1024;
#[cfg(target_os = "macos")]
const OPENLESS_BUNDLE_ID: &str = "com.openless.app";

/// 第一次 show 时把 QA 浮窗摆到屏幕底部居中；之后的 show 不再 reposition，
/// 让用户拖动后的位置在 hide → show 之间得以保持。详见 issue #118 v2。
static QA_WINDOW_POSITIONED: AtomicBool = AtomicBool::new(false);
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
// 桌面专用：移动端 WebviewWindowBuilder 没有 decorations/shadow 等方法，懒创建只在桌面用。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[cfg(not(mobile))]
use crate::types::{PolishMode, StylePack, StylePackKind};

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
            commands::get_settings,
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
            commands::marketplace_logout,
            commands::list_vocab,
            commands::add_vocab,
            commands::remove_vocab,
            commands::set_vocab_enabled,
            commands::list_correction_rules,
            commands::add_correction_rule,
            commands::remove_correction_rule,
            commands::set_correction_rule_enabled,
            commands::list_vocab_presets,
            commands::save_vocab_presets,
            commands::start_dictation,
            commands::stop_dictation,
            commands::cancel_dictation,
            commands::resume_cancelled_recording,
            commands::dismiss_cancelled_recording_recovery,
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
            commands::set_style_pack_hotkeys,
            commands::qa_window_dismiss,
            commands::qa_toggle_recording,
            commands::qa_submit_text,
            commands::qa_set_edit_instruction_mode,
            commands::less_computer_window_dismiss,
            commands::less_computer_window_open,
            commands::chat_panel_focus_keyboard,
            commands::less_computer_submit_text,
            commands::less_computer_sync,
            commands::less_computer_approve,
            commands::validate_combo_hotkey,
            commands::set_combo_hotkey,
            commands::validate_provider_credentials,
            commands::list_provider_models,
            commands::local_asr_get_settings,
            commands::local_asr_storage_settings,
            commands::local_asr_set_models_base_dir,
            commands::local_asr_set_active_model,
            commands::local_asr_set_mirror,
            commands::local_asr_list_models,
            commands::local_asr_fetch_remote_info,
            commands::local_asr_fetch_hf_card,
            commands::local_asr_download_model,
            commands::local_asr_cancel_download,
            commands::local_asr_delete_model,
            commands::local_asr_model_dir,
            commands::local_asr_reveal_model_dir,
            commands::local_asr_reveal_models_root,
            commands::local_asr_test_model,
            commands::local_asr_engine_status,
            commands::local_asr_release_engine,
            commands::local_asr_preload,
            commands::local_asr_set_keep_loaded_secs,
            commands::foundry_local_asr_status,
            commands::foundry_local_asr_catalog,
            commands::foundry_local_asr_set_model,
            commands::foundry_local_asr_set_language_hint,
            commands::foundry_local_asr_set_runtime_source,
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
            $crate::commands::get_settings,
            $crate::commands::get_default_style_system_prompts,
            $crate::commands::set_settings,
            $crate::commands::check_network,
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
            $crate::commands::validate_provider_credentials,
            $crate::commands::list_provider_models,
            $crate::commands::list_history,
            $crate::commands::delete_history_entry,
            $crate::commands::clear_history,
            $crate::commands::get_activity_stats,
            $crate::commands::read_audio_recording,
            $crate::commands::export_audio_recording,
            $crate::commands::retranscribe_recording,
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
            $crate::commands::marketplace_logout,
            $crate::commands::list_vocab,
            $crate::commands::add_vocab,
            $crate::commands::remove_vocab,
            $crate::commands::set_vocab_enabled,
            $crate::commands::list_correction_rules,
            $crate::commands::add_correction_rule,
            $crate::commands::remove_correction_rule,
            $crate::commands::set_correction_rule_enabled,
            $crate::commands::list_vocab_presets,
            $crate::commands::save_vocab_presets,
            $crate::commands::start_dictation,
            $crate::commands::stop_dictation,
            $crate::commands::cancel_dictation,
            $crate::commands::resume_cancelled_recording,
            $crate::commands::dismiss_cancelled_recording_recovery,
            $crate::commands::qa_window_dismiss,
            $crate::commands::qa_toggle_recording,
            $crate::commands::qa_submit_text,
            $crate::commands::qa_set_edit_instruction_mode,
            $crate::commands::repolish,
            $crate::commands::list_style_packs,
            $crate::commands::create_style_pack_from_template,
            $crate::commands::save_style_pack,
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
    let foundry_local_runtime = Arc::new(asr::local::FoundryLocalRuntime::new());
    let sherpa_onnx_runtime = Arc::new(asr::local::SherpaOnnxRuntime::new());
    let sherpa_download_manager =
        Arc::new(asr::local::sherpa_download::SherpaDownloadManager::new());
    #[cfg(target_os = "windows")]
    let coordinator = Arc::new(coordinator::Coordinator::new_with_local_runtimes(
        Arc::clone(&foundry_local_runtime),
        Arc::clone(&sherpa_onnx_runtime),
    ));
    #[cfg(not(target_os = "windows"))]
    let coordinator = Arc::new(coordinator::Coordinator::new());
    #[cfg(target_os = "windows")]
    if let Err(error) = coordinator.sync_active_asr_provider_from_preferences() {
        log::warn!("[startup] sync active ASR provider from preferences failed: {error}");
    }
    let local_asr_download_manager = Arc::new(asr::local::DownloadManager::new());

    let builder = tauri::Builder::default();
    // macOS：胶囊要叠到别的 app 的全屏 Space 之上，必须是「非激活 NSPanel」(普通
    // NSWindow 即便设 collectionBehavior 也做不到 —— tauri#9556 / #11488)。下面 setup 里
    // 的 capsule.to_panel() 依赖本插件注册的 panel 注册表；插件仅 macOS。
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    builder
        // 单实例锁：第二个进程启动时立即退出，激活信号转给已运行实例的主窗口。
        // 否则两份 OpenLess（如 /Applications/ + dev build）会各自抓全局热键，
        // 导致按一次键、两个进程同时跑流水线、文本被插入两遍。见 issue #50。
        //
        // 第二个进程的 argv 还有一个用处：作为 Linux 下的「触发器入口」。
        // 桌面环境快捷键执行 `openless --toggle-dictation` 时，第二个进程被本插件
        // 拦截 → argv 直接转给主实例 coordinator。详见 issue #420 / `cli.rs`。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(intent) = cli::parse_cli_intent(&argv) {
                log::info!(
                    "[single-instance] another instance launched with intent={intent:?}, dispatching"
                );
                dispatch_cli_intent(app, intent);
                return;
            }
            // 静默启动模式下：第二次启动（Win11 的「登录时重新打开应用」、autostart 双触发、
            // 或用户手动再点图标）也不弹主窗口，否则 start_minimized=true 在 Win11 上整体失效。
            // 用户想看主窗口走托盘菜单 / 托盘左键。issue #468。
            if let Some(coordinator) = app
                .try_state::<Arc<coordinator::Coordinator>>()
                .map(|s| Arc::clone(&*s))
            {
                if coordinator.prefs().get().start_minimized {
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
        // 跨平台开机自启：mac 写 LaunchAgent plist，linux 写 ~/.config/autostart/*.desktop，
        // windows 写 HKCU\Software\Microsoft\Windows\CurrentVersion\Run。前端 toggle 直接
        // 调插件 isEnabled / enable / disable，不维持本地 prefs，让 OS 当唯一真相。issue #194。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(coordinator.clone())
        .manage(local_asr_download_manager.clone())
        .manage(sherpa_download_manager.clone())
        .manage(foundry_local_runtime.clone())
        .manage(sherpa_onnx_runtime.clone())
        .manage(commands::MicrophoneMonitorState::new(None))
        .manage(commands::TrayMicrophoneMenuState::new(Vec::new()))
        .manage(TrayMicrophoneDeviceCache(parking_lot::Mutex::new(Vec::new())))
        .setup(move |app| {
            init_file_logger();
            log::info!("=== OpenLess 启动 ===");

            #[cfg(target_os = "windows")]
            if let Err(err) =
                crate::windows_ime_profile::apply_windows_openless_keyboard_list_pref(
                    &coordinator.prefs().get(),
                )
            {
                log::warn!(
                    "[windows-ime] apply keyboard list visibility pref on startup failed: {err}"
                );
            }

            // Capsule 启动时定位到屏幕底部居中并隐藏；coordinator 按需显示。
            // 与 Swift `CapsuleWindowController.repositionToBottomCenter` 同语义。
            if let Some(capsule) = app.get_webview_window("capsule") {
                // macOS：转成「非激活 NSPanel」，否则胶囊叠不到别的 app 的全屏之上
                // （普通 NSWindow 只靠 collectionBehavior 做不到 —— tauri#9556 / #11488）。
                #[cfg(target_os = "macos")]
                {
                    use tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior;
                    use tauri_nspanel::WebviewWindowExt;
                    match capsule.to_panel() {
                        Ok(panel) => {
                            // 非激活：显示/点击都不激活本 app、不切走当前(含全屏)Space。
                            const NS_NONACTIVATING_PANEL_MASK: i32 = 1 << 7;
                            panel.set_style_mask(NS_NONACTIVATING_PANEL_MASK);
                            // 抬到菜单栏(24)之上。
                            panel.set_level(25);
                            // 加入所有 Space + 作为辅助窗口出现在全屏 app 的 Space 上。
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
                // Linux 下 tao 的鼠标穿透实现会直接解包底层 GDK 窗口；visible=false 时
                // 窗口尚未 realize。这里只创建窗口系统资源而不 map，避免 X11 崩溃，
                // 也不会像 show() 那样在不支持窗口定位的 Wayland 上造成启动闪窗。
                #[cfg(target_os = "linux")]
                let cursor_passthrough_ready = match capsule.gtk_window() {
                    Ok(gtk_window) => {
                        gtk_window.realize();
                        true
                    }
                    Err(e) => {
                        log::warn!("[capsule] gtk_window failed; skipping cursor passthrough: {e}");
                        false
                    }
                };
                #[cfg(not(target_os = "linux"))]
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

            // QA / Less Computer / glow 浮窗改为懒创建（不再在 tauri.conf.json eager 声明）：
            // 用到时才 build（ensure_qa_window / ensure_less_computer_window /
            // ensure_less_computer_glow_window），idle 时根本没有它们的 WebKit 进程 ——
            // 省 3 个常驻 webview。定位 + QA 拖拽修复在创建/show 路径里补。

            // 主窗口磨砂：macOS 用 NSVisualEffectView，Windows 用 Mica。
            // 没这一层的话 transparent: true 让窗口透明 → 背后只是空，不是磨砂。
            //
            // decorations 留给运行时分平台决定：macOS 默认 true 用系统红黄绿；
            // Windows 这里关掉 native chrome 让 React 端 WinTitleBar 接管。
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
                    // Windows 走 Tauri decorations:true 原生 Win11 标题栏 / 关闭按钮 /
                    // 拖动 / 圆角 / resize border。保留 apply_mica 给原生 chrome 提供
                    // 磨砂材质，配合 WindowChrome 半透明 background 让 sidebar 透出玻璃感。
                    if let Err(e) = apply_mica(&main, None) {
                        log::warn!("[main] mica failed: {e}");
                    }
                    // Win11 22H2+: 同步原生标题栏主题；前端就绪后会再调 set_windows_caption_theme。
                    // 老版 Windows 静默失败，不阻塞。
                    apply_windows_caption_theme(&main, false);
                }
                // 静默启动开关：prefs.start_minimized = true → 不弹主窗口，
                // 用户从菜单栏 / 托盘点击访问。开机自启时尤其有用，避免每次
                // 登录都被主窗口打扰。OPENLESS_SHOW_MAIN_ON_START=1 仍保留
                // 老的强制 show 路径（手动 dispatch 测试 / dev 用），优先级高
                // 于 prefs。
                let force_show =
                    std::env::var("OPENLESS_SHOW_MAIN_ON_START").ok().as_deref() == Some("1");
                let suppress_show = !force_show && coordinator.prefs().get().start_minimized;
                if suppress_show {
                    log::info!("[main] start_minimized=true → 跳过初始 show，等用户点托盘");
                } else {
                    #[cfg(target_os = "linux")]
                    {
                        // Workaround for Linux Wayland WebKitGTK compositing:
                        // `visible:false` → `show()` can leave the webview surface
                        // without a valid input region. The ±1px nudge forces
                        // GTK size-allocate → input surface reattach.
                        // Ref: tauri#9394, cc-switch linux_fix.rs
                        let main_clone = main.clone();
                        let _ = main_clone.set_focus();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            let _ = main_clone.set_focus();
                            if let Ok(orig) = main_clone.inner_size() {
                                let bumped = tauri::PhysicalSize::new(
                                    orig.width.saturating_add(1),
                                    orig.height,
                                );
                                let _ = main_clone.set_size(bumped);
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                let _ = main_clone.set_size(orig);
                                log::info!("[main] Linux nudge: focus + surface reactivation done");
                                // Reconcile: compositor may have coalesced the two
                                // set_size calls, leaving the window at width+1.
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                if let Ok(after) = main_clone.inner_size() {
                                    // Only correct the ±1px nudge artifact — if the
                                    // compositor or user resized the window significantly
                                    // during this window, don't clobber that change.
                                    let dw = if after.width > orig.width { after.width - orig.width } else { orig.width - after.width };
                                    let dh = if after.height > orig.height { after.height - orig.height } else { orig.height - after.height };
                                    if dw <= 1 && dh <= 1 && (dw > 0 || dh > 0) {
                                        let _ = main_clone.set_size(orig);
                                    }
                                }
                            }
                        });
                    }
                    if let Err(e) = main.show() {
                        log::warn!("[main] initial show failed: {e}");
                    }
                }
            }

            // Accessibility 授权不再在 setup() 中触发。
            //
            // 原因：setup() 在 AppKit event loop 就绪之前执行，此时
            // AXIsProcessTrustedWithOptions 调用的 XPC 通信依赖 run loop，
            // 在部分 macOS 版本上可能导致 TCC 弹窗不弹出但已被 TCC 标记为
            // "已展示"——之后前端 onboarding 再次调用时不再弹窗，只能走系统设置+
            // 重启恢复，用户就会卡死在 onboarding 页。
            //
            // 现在由前端 onboarding 页面在用户点击「授权辅助功能」按钮时触发，
            // 符合 Apple HIG："在解释用途之后再弹出权限请求"。
            // 已授权用户不受影响（AXIsProcessTrusted 返回 true，引导页直接跳过）。

            // AppImage / 便携版：fcitx5 插件缺了就从 bundled resources 自动安装
            // 到 ~/.local/ 下面。不会覆盖系统已有的插件。
            #[cfg(target_os = "linux")]
            crate::linux_fcitx::ensure_plugin_installed(app.handle());

            // 菜单栏图标 — 与 Swift `MenuBarController` 同语义：
            // 左键点 → 显示/聚焦主窗口；菜单含「显示主窗口」「退出」。
            let tray_menu = build_tray_menu(app, &coordinator)?;
            let menu = tray_menu.menu;

            // 与 Swift `StatusBarIcon.swift` 行为一致：用全彩 AppIcon，**不**走 template 模式
            // （走 template 会被 macOS 染成单色 → 看起来像个黑方块）。
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

            // Spin up hotkey listener; coordinator owns the lifecycle.
            let app_handle = app.handle().clone();
            coordinator.bind_app(app_handle);
            coordinator.start_hotkey_listener();
            // QA / custom combo hotkeys use `global-hotkey` (Carbon on macOS).
            // Start those after RunEvent::Ready, when the AppKit event loop is live.
            if std::env::var("OPENLESS_SHOW_MAIN_ON_START").ok().as_deref() == Some("1") {
                show_main_window(app.handle());
            }

            // 首次启动也可能带 CLI flag（用户双击 .desktop 之前先用 CLI 起一遍）。
            // 等 coordinator 准备好后再 dispatch；GUI 仍然照常起来。
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
                // 同步启动 QA hotkey listener。和 dictation hotkey 平行，互不抢状态。
                coordinator.start_qa_hotkey_listener();
                coordinator.start_selection_polish_hotkey_listener();
                // 选区语音复用选区润色热键，不再单独注册 voice hotkey。
                // 启动「快速 Agent」双热键监听（功能默认关闭，启用后才注册）。
                coordinator.start_coding_agent_hotkey_listener();
                // 启动自定义组合键监听器。当 trigger == Custom 时替代 modifier-only 监听器。
                coordinator.start_combo_hotkey_listener();
                coordinator.start_translation_hotkey_listener();
                coordinator.start_switch_style_hotkey_listener();
                coordinator.start_open_app_hotkey_listener();
                coordinator.start_style_pack_hotkey_listeners();
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
                coordinator.stop_style_pack_hotkey_listeners();
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
    let labels = TrayLabels::for_locale(&coordinator.remote_locale());
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
    let prefs = coordinator.prefs().get();
    let packs = coordinator.style_packs().list().unwrap_or_else(|err| {
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
    let selected = coordinator.prefs().get().microphone_device_name;
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

/// 在主线程上刷新托盘麦克风子菜单并通知前端。供 OS 原生设备变更回调与慢速兜底轮询
/// 共用同一条收尾路径。已在主线程或被 `run_on_main_thread` 派发后调用。
#[cfg(not(mobile))]
fn refresh_microphone_on_main(app: &AppHandle) {
    if let Err(err) = refresh_tray_microphone_menu(app) {
        log::warn!("[tray] refresh microphone menu after device change failed: {err}");
    }
    let _ = app.emit("microphone:devices-changed", serde_json::json!({}));
}

/// 设备变更去抖闭包：被 OS 原生通知回调（macOS CoreAudio / Windows MMDevice）调用。
/// OS 回调仅调度一个后台枚举任务，绝不在 AppKit 主线程或 CoreAudio/COM 通知线程中
/// 直接枚举设备。并发通知通过 `refresh_in_flight` 合并，签名去抖避免重复刷新菜单。
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
    //    Linux 无原生路径，返回 false，纯靠下面的慢速兜底。
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

    // 2) 全平台慢速兜底：60s 无条件轮询，复用 signature 去抖（签名没变就 continue，零
    //    副作用）。原生通知失败时由它保证设备变更最终被检测到；原生通知正常时它只是
    //    极低频的安全网，几乎从不真正刷新。
    if let Err(err) = std::thread::Builder::new()
        .name("openless-tray-mic-poll".into())
        .spawn(move || {
            let last_signature = parking_lot::Mutex::new(None);
            // Populate the initially empty tray cache without blocking AppKit startup.
            refresh_microphone_cache_if_changed(&app, &last_signature);
            while !TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                // 60s（而非 10s）：原生通知承担实时检测，这条线程只是兜底，把它拉到 60s
                // 进一步压低空闲唤醒。1s 一片的睡眠让退出 flag 最多 1s 内生效，避免退出时
                // 长时间挂起线程。
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
    let mut prefs = coord.prefs().get();
    prefs.microphone_device_name = selected.device_name.clone();
    if let Err(err) = coord.prefs().set(prefs.clone()) {
        log::warn!("[tray] save microphone preference failed: {err}");
        return;
    }
    let _ = app.emit("prefs:changed", &prefs);

    commands::sync_tray_microphone_selection(&items, &selected.device_name);
}

#[cfg(not(mobile))]
fn handle_style_tray_menu_event(app: &AppHandle, id: &str) -> bool {
    let Some(pack_id) = parse_tray_style_pack_menu_id(id) else {
        return false;
    };
    let coord = app.state::<Arc<coordinator::Coordinator>>();
    let packs = match coord.style_packs().list() {
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

/// Win11 22H2+ (Build 22621+) 同步原生标题栏沉浸式暗色 / caption / text / border 色。
/// 老 Windows 上 DwmSetWindowAttribute 返回错误，仅打 warn 不阻塞启动。
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

    // COLORREF 0x00BBGGRR — light 对齐 WindowChrome glass 起始色 rgb(245,245,247)；
    // dark 对齐 tokens.css --ol-surface (#141922) / --ol-ink (#f4f7fb) / --ol-surface-2 (#1a202b)。
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

/// 前端主题切换时同步主窗口原生标题栏；非 Windows 为 no-op。
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
    // macOS：自动更新会让新装的 .app 带 com.apple.quarantine（无论 Tauri updater
    // 怎么解包，下载流由 LaunchServices 接管，输出物可能仍带 xattr）。如果不
    // strip，重启后 Gatekeeper 会拦着说"OpenLess 已损坏 / 来自未识别开发者"，
    // 用户必须自己开终端跑 xattr -cr 才能继续用 — 违反了"自动更新对用户应该零摩擦"。
    //
    // 在 restart 前阻塞地清一次 xattr。失败容忍（PATH 异常、xattr 不存在、磁盘
    // 只读等边角情况），不让它阻塞重启本身。
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

/// 把前端的关键错误（如自动更新 install 失败）转发到 Rust 文件日志（openless.log）。
/// webview 的 console.error 不会进 openless.log，单独留一个 IPC，便于用户「导出日志」
/// 后我们拿到自动更新失败的真实原因。
#[tauri::command]
fn log_client_error(message: String) {
    // message 由前端 webview 可控，可能很长或含换行（伪造日志行）。先把换行折成空格、
    // 再按 UTF-8 字符边界截断，避免单条日志过大或污染日志格式。
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

/// 把日志同时写到 stderr + ~/Library/Logs/OpenLess/openless.log（match Swift `Log.swift`）。
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

/// 把 CLI intent 路由到 coordinator。两个入口共用：
/// 1. 首次启动（lib.rs setup 末尾）
/// 2. single-instance 回调（第二个进程被拦截后转发 argv）
///
/// 异步动作（start_dictation / stop_dictation 是 async）通过 tauri 自带 runtime spawn，
/// 不阻塞回调线程。所有动作都按 coordinator 当前状态自检：
/// - ToggleDictation 在 Idle → start，在 Listening → stop，Starting/Processing/Inserting 忽略并记日志
/// - ToggleQa 直接转发到 handle_qa_hotkey_pressed（语义等同于按一次 QA 热键）
/// - CancelDictation 直接调 cancel（cancel 本身在非 Listening 时也安全）
fn dispatch_cli_intent<R: Runtime>(app: &AppHandle<R>, intent: cli::CliIntent) {
    let coordinator = app
        .try_state::<Arc<coordinator::Coordinator>>()
        .map(|s| Arc::clone(&*s));
    let Some(coordinator) = coordinator else {
        log::warn!("[cli] coordinator not yet managed; dropping intent={intent:?}");
        return;
    };
    match intent {
        cli::CliIntent::ToggleDictation => {
            let coord = Arc::clone(&coordinator);
            tauri::async_runtime::spawn(async move {
                let phase = coord.dictation_phase_for_cli();
                use coordinator_state::SessionPhase;
                match phase {
                    SessionPhase::Idle => {
                        log::info!("[cli] toggle-dictation: Idle → start_dictation");
                        if let Err(e) = coord.start_dictation().await {
                            log::warn!("[cli] start_dictation failed: {e}");
                        }
                    }
                    SessionPhase::Listening => {
                        log::info!("[cli] toggle-dictation: Listening → stop_dictation");
                        if let Err(e) = coord.stop_dictation().await {
                            log::warn!("[cli] stop_dictation failed: {e}");
                        }
                    }
                    SessionPhase::Starting => {
                        // 复用 stop_dictation 自身的 Starting → pending_stop 处理，
                        // 与按一次主热键的行为对齐（issue #51）。
                        log::info!("[cli] toggle-dictation: Starting → stop_dictation (pending)");
                        if let Err(e) = coord.stop_dictation().await {
                            log::warn!("[cli] stop_dictation failed: {e}");
                        }
                    }
                    other => {
                        log::info!("[cli] toggle-dictation ignored (phase={other:?})");
                    }
                }
            });
        }
        cli::CliIntent::ToggleQa => {
            let coord = Arc::clone(&coordinator);
            tauri::async_runtime::spawn(async move {
                log::info!("[cli] toggle-qa: dispatching to qa hotkey handler");
                coord.cli_toggle_qa_panel().await;
            });
        }
        cli::CliIntent::CancelDictation => {
            log::info!("[cli] cancel-dictation: invoking cancel");
            coordinator.cancel_dictation();
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

/// 展示胶囊后调用：若 OpenLess 已是前台 app，用 makeKeyWindow 还原主窗口焦点。
/// 不调 NSApp.activate，不抢其他 app 焦点，符合 CLAUDE.md 约束。
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

/// QA 浮窗的目标尺寸（issue #118；统一聊天面板后与 Less Computer 同尺寸）。
/// 窗口固定大小，内容在面板内部的 MessageScroller 里滚动。
const QA_WINDOW_WIDTH: f64 = 420.0;
const QA_WINDOW_HEIGHT: f64 = 540.0;
/// 胶囊与 QA 窗口的间距，与设计稿一致。
const QA_WINDOW_GAP_TO_CAPSULE: f64 = 8.0;
/// 给 macOS Dock 留的下边距（与 capsule 同源）。
const DOCK_BOTTOM_PADDING_FOR_QA: f64 = 80.0;

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

fn bottom_visual_position(
    frame: LogicalMonitorFrame,
    window_width: f64,
    visual_height: f64,
    bottom_padding: f64,
    bottom_inset: f64,
) -> (f64, f64) {
    let x = frame.x + ((frame.width - window_width) / 2.0).max(0.0);
    let y = frame.y + (frame.height - visual_height - bottom_padding - bottom_inset).max(0.0);
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

/// 胶囊目标显示器快照：物理矩形 + DPI 缩放。
///
/// macOS 下由当前 focused input / caret 所在位置映射而来，供实际定位与
/// capsule layout cache 共用。用 Tauri monitor 的物理坐标作为稳定 key；
/// 真正 set_position 前再转成逻辑坐标，避免 Retina 下窗口尺寸翻倍。
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CapsuleTargetMonitor {
    pub(crate) physical_x: i32,
    pub(crate) physical_y: i32,
    pub(crate) physical_width: u32,
    pub(crate) physical_height: u32,
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
}

/// macOS：决定胶囊应该摆到哪块显示器。
///
/// 跟随**鼠标光标所在的屏**——这是用户的操作/视线焦点，也是唯一始终可用、
/// 无需任何权限的信号，多显示器 + 多 Space 下都能稳定命中。光标取不到时
/// （理论上不会）才退回 AX focused-input/caret 位置。
///
/// 关键：不能用 capsule window 自己的 current_monitor——窗口隐藏时它仍停留在
/// 上一次出现的屏，多屏会被缓存误判为“不需要移动”，把胶囊锁死在第一块屏。
/// 选屏时先找包含该点的屏；点短暂落在所有屏外则退到最近的屏，避免虚拟桌面
/// 负坐标 / 屏幕排列边缘导致完全不显示。定位与 layout 去重缓存共用本函数，
/// 二者看的必须是同一块屏。
#[cfg(target_os = "macos")]
pub(crate) fn capsule_target_monitor<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Option<CapsuleTargetMonitor> {
    let (x, y) = macos_mouse_cursor_point().or_else(macos_focused_input_anchor_point)?;
    monitor_for_anchor_point(window, x, y)
}

/// 在 Tauri 的 monitor 坐标系里，选出包含逻辑坐标点 `(x, y)` 的显示器；
/// 点落在所有屏之外时退到最近的屏。坐标系同 AX / CGEvent 的全局显示空间
/// （左上原点，points）。
#[cfg(target_os = "macos")]
fn monitor_for_anchor_point<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    x: f64,
    y: f64,
) -> Option<CapsuleTargetMonitor> {
    let monitors = window.available_monitors().ok()?;
    let mut nearest: Option<(f64, CapsuleTargetMonitor)> = None;

    for monitor in monitors {
        let target = CapsuleTargetMonitor {
            physical_x: monitor.position().x,
            physical_y: monitor.position().y,
            physical_width: monitor.size().width,
            physical_height: monitor.size().height,
            scale: monitor.scale_factor(),
        };
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

    /// 当前鼠标光标在「全局显示坐标系」(左上原点，points) 的位置。该坐标系与 AX
    /// caret 完全一致，可直接拿去和 `logical_frame` 比较选屏。`CGEventGetLocation`
    /// 始终可用、不需要任何权限，所以作为胶囊跟随屏幕的首选信号。
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

/// 把窗口左上角 `(x, y)`（同 area 同坐标系，physical px）夹到给定矩形内，
/// **保证整窗（含自身 w×h）落在 area 内可见**。area 为工作区时即可避开任务栏。
///
/// 纯函数，无 Win32 依赖，便于单测多显示器 / 负原点 / 异常 DPI 输入。issue #470：
/// 此前 Windows 分支只夹上边（`y.max(mon.top)`），左/右/下未夹，多屏负坐标下胶囊
/// 可能被算到屏外却无任何观测。这里四边都夹。
///
/// area 比窗口还小时（`area_right - w < area_left`），`max_x` 退化为 `area_left`，
/// `clamp` 把左上角收回 area 左上角，保证至少左上角可见、不溢出为负超界。
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
    // 右/下边界 = area 右下角减去窗口自身尺寸，确保整窗可见。
    // 用 saturating_sub 防 area_right/area_bottom 为极小（含 i32::MIN 近邻）时减法溢出。
    let max_x = area_right.saturating_sub(w).max(area_left);
    let max_y = area_bottom.saturating_sub(h).max(area_top);
    let clamped_x = x.clamp(area_left, max_x);
    let clamped_y = y.clamp(area_top, max_y);
    (clamped_x, clamped_y)
}

/// 把 QA 浮窗放到屏幕底部居中、紧贴胶囊上方。tauri 启动期 + show 之前都会调一次，
/// 防止用户切换显示器后位置错乱。
fn position_qa_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    let monitor = match window.current_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let pos = monitor.position();
    let frame = logical_monitor_frame(pos.x, pos.y, size.width, size.height, scale);
    let capsule_height = capsule_height_for_qa();
    let (x, y) = bottom_center_position(
        frame,
        QA_WINDOW_WIDTH,
        QA_WINDOW_HEIGHT,
        DOCK_BOTTOM_PADDING_FOR_QA + capsule_height + QA_WINDOW_GAP_TO_CAPSULE,
    );
    window.set_size(tauri::LogicalSize::new(QA_WINDOW_WIDTH, QA_WINDOW_HEIGHT))?;
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// 显示 QA 窗口并发一条状态事件（前端订阅 `qa:state`）。
/// `content_kind` 是不透明字符串（"loading" / "answer" / "idle" 等），
/// 让前端 React 视图自行决定渲染哪一种。**不**抢前台 app 焦点（保证 Cmd+C
/// fallback 仍能从原 app 拿到选区）。
pub(crate) fn show_qa_window<R: tauri::Runtime>(app: &AppHandle<R>, content_kind: &str) {
    #[cfg(target_os = "android")]
    {
        const FLAG_ACTIVITY_NEW_TASK: i32 = 0x10000000;
        const FLAG_ACTIVITY_REORDER_TO_FRONT: i32 = 0x00020000;
        const FLAG_ACTIVITY_SINGLE_TOP: i32 = 0x20000000;
        let flags =
            FLAG_ACTIVITY_NEW_TASK | FLAG_ACTIVITY_REORDER_TO_FRONT | FLAG_ACTIVITY_SINGLE_TOP;
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::start_activity_class_with_flags(
                env,
                context,
                "com.openless.app.MainActivity",
                flags,
            )
        }) {
            Ok(()) => log::info!("[qa] android requested MainActivity foreground for QA"),
            Err(error) => log::warn!("[qa] android failed to foreground MainActivity: {error}"),
        }
        log::info!("[qa] android emit qa:state to main kind={content_kind}");
        let _ = app.emit_to(
            "main",
            "qa:state",
            serde_json::json!({ "kind": content_kind }),
        );
        return;
    }

    let Some(window) = ensure_qa_window(app) else {
        log::info!("[qa] show 跳过：qa 窗口不存在 (content_kind={content_kind})");
        return;
    };
    // 仅首次 show 时居中；之后保留用户拖动后的位置。
    if !QA_WINDOW_POSITIONED.load(Ordering::Relaxed) {
        if let Err(e) = position_qa_window(&window) {
            log::warn!("[qa] position before first show failed: {e}");
        }
        QA_WINDOW_POSITIONED.store(true, Ordering::Relaxed);
    }
    // macOS：不用 window.show()（它会 makeKeyAndOrderFront 把 OpenLess 推成 frontmost，
    // 之后 capture_selection 的 AX read / Cmd+C fallback 都跑在 OpenLess 自己的 webview 上
    // → 抓不到原 app 选区）。改用 orderFrontRegardless 让窗口可见但**不**成为 key window，
    // frontmost 仍是用户原 app，AX 还能读到选区。这是 Spotlight / Raycast 的标准做法。
    //
    // ⚠️ 关键：NSWindow 任何操作必须在主线程，macOS 26 是硬断言（违反直接 SIGTRAP）。
    // show_qa_window 经常从 tokio worker 调（qa_hotkey_bridge_loop），所以裸 ObjC msg_send
    // 必须用 `app.run_on_main_thread` dispatch 到主线程。详见 issue #118 v2。
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
    // 作废挂起的退场 hide（快速关-开），并让前端重放入场动画。
    QA_PANEL_EPOCH.fetch_add(1, Ordering::SeqCst);
    let _ = app.emit_to("qa", "chat-panel:shown", serde_json::json!({}));
    let _ = app.emit_to(
        "qa",
        "qa:state",
        serde_json::json!({ "kind": content_kind }),
    );
}

/// 聊天面板浮窗（qa / less-computer）转「非激活 NSPanel」（macOS，胶囊同手法）。
///
/// 目的：让面板可以**在 OpenLess 不激活的情况下成为 key window**（Spotlight 同款）。
/// 之前打字焦点走 `window.set_focus()`，它会激活整个 app —— 主窗口（设置页）被
/// 一起带到前台，且 frontmost 变成 OpenLess 自己、AX 再也读不到原 app 的选区。
/// 转成 NonactivatingPanel 后：点输入框 / makeKeyWindow 只给面板键盘焦点，
/// app 保持后台、主窗口不动、frontmost 始终是用户原 app。
/// 顺带 CanJoinAllSpaces + FullScreenAuxiliary：划词常发生在全屏 app 里，面板要能叠上去。
#[cfg(target_os = "macos")]
fn make_chat_window_panel_macos<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, tag: &str) {
    use tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior;
    use tauri_nspanel::WebviewWindowExt;
    match window.to_panel() {
        Ok(panel) => {
            const NS_NONACTIVATING_PANEL_MASK: i32 = 1 << 7;
            panel.set_style_mask(NS_NONACTIVATING_PANEL_MASK);
            // 浮层级别（NSFloatingWindowLevel）：盖普通窗口，不盖菜单栏/胶囊(25)。
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

/// 聊天面板浮窗（qa / less-computer）的拖动修复（macOS）。
///
/// 配置 `focus: false` 让 Tauri 把窗口创建为 nonactivating panel 风格（避免抢前台 app
/// 焦点）。代价是 AppKit 的 `performWindowDragWithEvent:` 在 nonactivating 窗口上无效，
/// 所以 `data-tauri-drag-region` 和 `WebviewWindow::start_dragging()` 都拖不动。
///
/// 解法是把 NSWindow 的 `movableByWindowBackground` 打开——这条路径不依赖窗口是否成为
/// key window，跟 Spotlight / Raycast 的浮窗是同一手法。设一次就够，整个生命周期保持。
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
    // 异常兜底：AppKit 在 drag margins 状态不一致时会从 setMovable 内部 raise
    // NSException；不捕获的话异常穿过 Rust 边界直接 abort 整个 app。捕获后降级为
    // 日志（拖动失效但 app 活着），日志会指认具体异常便于追根因。
    // SAFETY: 闭包内只有两个无返回值的 ObjC 消息发送，没有需要运行析构的 Rust 值，
    // 异常展开跳过闭包帧不会破坏内存安全。
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

/// 懒创建 QA 浮窗：原来在 tauri.conf.json eager 创建（常驻一个 WebKit 进程）。改为首次
/// show 时才 build —— idle 时根本不存在 → 省一个常驻 webview。配置与原 tauri.conf 的 qa
/// 块逐项一致（"center": false ⇒ **不**调 .center()；"focus": false ⇒ focused(false)）。
/// 关键：make_qa_window_draggable_macos 原先只在启动时设一次，这里创建时必须补回，否则
/// 懒创建的 QA 窗口在 macOS 上拖不动。
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
            // ⚠️ NSWindow 操作必须在主线程（macOS 26 硬约束）。ensure_qa_window 常从
            // tokio worker 进来（热键桥接循环），在 worker 线程直接 setMovable 会让
            // AppKit 从 _postWindowNeedsToResetDragMargins 抛 NSException 直接崩掉
            // 整个 app（2026-07-03 四次同栈崩溃的根因）——dispatch 回主线程执行。
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

// 移动端 QA 路由到 main 窗口（show_qa_window 在 Android 早返回）；Android 的
// WebviewWindowBuilder 没有桌面方法，这里只占位返回已有窗口（编译用，运行时不达）。
#[cfg(any(target_os = "android", target_os = "ios"))]
fn ensure_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<tauri::WebviewWindow<R>> {
    app.get_webview_window("qa")
}

/// 懒创建 Less Computer 浮窗（macOS only）。配置与原 tauri.conf 的 less-computer 块一致。
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
    .resizable(false)
    .focused(false)
    .visible(false)
    .accept_first_mouse(true)
    .build()
    {
        Ok(w) => {
            // 与 QA 同款：转非激活 NSPanel（打字不激活 app）+ 拖动修复
            // （movableByWindowBackground；必须主线程，见 make_chat_window_draggable_macos）。
            let w_clone = w.clone();
            let _ = app.run_on_main_thread(move || {
                make_chat_window_panel_macos(&w_clone, "less-computer");
                make_chat_window_draggable_macos(&w_clone, "less-computer");
            });
            Some(w)
        }
        Err(e) => {
            log::warn!("[less-computer] lazy window create failed: {e}");
            None
        }
    }
}

/// 懒创建 Less Computer glow 描边窗（macOS only）。shadow:false、无 acceptFirstMouse。
/// 它的 level/collectionBehavior/ignore-mouse 在每次 show_less_computer_glow 里幂等设置，
/// 所以创建时不需要额外原生配置。
#[cfg(target_os = "macos")]
fn ensure_less_computer_glow_window<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    if let Some(w) = app.get_webview_window("less-computer-glow") {
        return Some(w);
    }
    match WebviewWindowBuilder::new(
        app,
        "less-computer-glow",
        WebviewUrl::App("index.html?window=less-computer-glow".into()),
    )
    .title("OpenLess Less Computer Glow")
    .inner_size(800.0, 600.0)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .build()
    {
        Ok(w) => Some(w),
        Err(e) => {
            log::warn!("[less-computer-glow] lazy window create failed: {e}");
            None
        }
    }
}

/// 带退场动画地隐藏聊天面板：先发 `chat-panel:closing` 让前端播退场动画，
/// 240ms 后真正 hide。期间再次 show（epoch 前进）则作废本次挂起的 hide ——
/// 避免「关的动画还没放完用户又唤起 → 窗口被旧定时器藏掉」。
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

/// 隐藏 QA 窗口。供 commands::qa_window_dismiss / coordinator session 收尾共用。
pub(crate) fn hide_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "android")]
    {
        let _ = app.emit_to("main", "qa:dismiss", serde_json::json!({}));
        return;
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    hide_chat_window_animated(app, "qa", &QA_PANEL_EPOCH);
}

/// 选区润色预览是独立、可编辑的小窗：模型结果不会直接覆盖，用户确认后才回到原选区粘贴。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn ensure_selection_polish_preview_window<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window("selection-polish-preview") {
        return Some(window);
    }
    WebviewWindowBuilder::new(
        app,
        "selection-polish-preview",
        WebviewUrl::App("index.html?window=selection-polish-preview".into()),
    )
    .title("OpenLess 选区润色预览")
    .inner_size(640.0, 440.0)
    .min_inner_size(480.0, 320.0)
    .resizable(true)
    .always_on_top(true)
    .visible(false)
    .build()
    .map(Some)
    .unwrap_or_else(|error| {
        log::warn!("[selection-polish] create preview window failed: {error}");
        None
    })
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn show_selection_polish_preview<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(window) = ensure_selection_polish_preview_window(app) else {
        return;
    };
    if let Err(error) = window.show() {
        log::warn!("[selection-polish] show preview failed: {error}");
        return;
    }
    if let Err(error) = window.set_focus() {
        log::warn!("[selection-polish] focus preview failed: {error}");
    }
    let _ = app.emit_to(
        "selection-polish-preview",
        "selection-polish-preview:shown",
        (),
    );
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn show_selection_polish_preview<R: tauri::Runtime>(_app: &AppHandle<R>) {}

pub(crate) fn hide_selection_polish_preview<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("selection-polish-preview") {
        let _ = window.hide();
    }
}

/// 选区语音：说完后由用户选择提问或编辑。
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
    .title("OpenLess 选区语音")
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
    let _ = app.emit_to(
        "selection-voice-intent",
        "selection-voice-intent:shown",
        (),
    );
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

// ───────────────────────── Less Computer 浮窗 ─────────────────────────
//
// Less Computer 语音 Agent 的聊天浮窗（窗口 label = "less-computer"）。
// 仅 macOS：和 coordinator / 前端对 Less Computer 的 gating 一致（Windows/Linux
// 不注册热键、前端 detectOS 不渲染入口），所以这些窗口操作全部 `#[cfg(macos)]`，
// 其它平台是 no-op，避免在非目标平台动 NSWindow / 弹一个空浮窗。

/// Less Computer 浮窗尺寸：与 QA 同款「统一聊天面板」固定大小 —— 窗口出现即
/// 定死，内容只在面板内部的 MessageScroller 里滚动，不再按内容自适应缩放窗口。
#[cfg(target_os = "macos")]
const LESS_COMPUTER_WINDOW_WIDTH: f64 = 420.0;
#[cfg(target_os = "macos")]
const LESS_COMPUTER_WINDOW_HEIGHT: f64 = 540.0;

/// 把 Less Computer 浮窗（固定尺寸）摆到屏幕底部居中、紧贴胶囊上方。
#[cfg(target_os = "macos")]
fn position_less_computer_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<()> {
    let monitor = match window.current_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let pos = monitor.position();
    let frame = logical_monitor_frame(pos.x, pos.y, size.width, size.height, scale);
    let capsule_height = capsule_height_for_qa();
    let (x, y) = bottom_center_position(
        frame,
        LESS_COMPUTER_WINDOW_WIDTH,
        LESS_COMPUTER_WINDOW_HEIGHT,
        DOCK_BOTTOM_PADDING_FOR_QA + capsule_height + QA_WINDOW_GAP_TO_CAPSULE,
    );
    window.set_size(tauri::LogicalSize::new(
        LESS_COMPUTER_WINDOW_WIDTH,
        LESS_COMPUTER_WINDOW_HEIGHT,
    ))?;
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// 显示 Less Computer 浮窗（不抢前台 app 焦点，与 QA 同手法）。`macos` 专用。
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
        if let Err(e) = position_less_computer_window(&window_clone) {
            log::warn!("[less-computer] position before show failed: {e}");
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
    // 作废挂起的退场 hide（快速关-开），并让前端重放入场动画。
    LESS_COMPUTER_PANEL_EPOCH.fetch_add(1, Ordering::SeqCst);
    let _ = app.emit_to("less-computer", "chat-panel:shown", serde_json::json!({}));
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_less_computer_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 隐藏 Less Computer 浮窗。供 dismiss 命令 / session 收尾共用。
#[cfg(target_os = "macos")]
pub(crate) fn hide_less_computer_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    hide_chat_window_animated(app, "less-computer", &LESS_COMPUTER_PANEL_EPOCH);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_less_computer_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 显示全屏彩虹描边浮层：盖满当前显示器、点击穿透、置顶。Agent 工作时点亮整屏边缘。
#[cfg(target_os = "macos")]
pub(crate) fn show_less_computer_glow<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(window) = ensure_less_computer_glow_window(app) else {
        return;
    };
    // 盖满当前（否则主）显示器，含菜单栏/Dock 区域。关键：用「逻辑坐标」(物理/缩放) ——
    // Retina 上 monitor.size() 是物理像素(2x)，直接 set_size 会把窗口铺成两倍、错位、不贴边。
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let pos = monitor.position();
        let _ = window.set_position(tauri::LogicalPosition::new(
            pos.x as f64 / scale,
            pos.y as f64 / scale,
        ));
        let _ = window.set_size(tauri::LogicalSize::new(
            size.width as f64 / scale,
            size.height as f64 / scale,
        ));
    }
    // 点击穿透：纯视觉浮层，绝不拦截鼠标。
    let _ = window.set_ignore_cursor_events(true);
    // issue #470：通知 glow 前端「可见」，恢复发光动画（隐藏时会 emit(false) 卸载发光层以释放 GPU）。
    let _ = window.emit("less-computer-glow:active", true);
    let window_clone = window.clone();
    let app_for_reassert = app.clone();
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        match window_clone.ns_window() {
            Ok(handle) => {
                let ns = handle as *mut AnyObject;
                if ns.is_null() {
                    let _ = window_clone.show();
                } else {
                    unsafe {
                        // 抬到菜单栏(24)/Dock 之上，让描边能真正贴到屏幕最外缘（含顶部菜单栏区域）。
                        let _: () = msg_send![ns, setLevel: 25i64];
                        // 所有 Space 都显示、不参与窗口循环、全屏 app 上也叠加（273 =
                        // CanJoinAllSpaces|Stationary|FullScreenAuxiliary）。macOS 26 会在
                        // 运行中把窗口从「全 Space 贴附」剥离且同值写入救不回（详见
                        // show_capsule_window_no_activate 的重注册注释）；glow show 频率低，
                        // 每次都走重注册序列：先以去掉 CanJoinAllSpaces 位的 272 上屏，
                        // 下一个 tick 再写 273 —— 可见状态下的位翻转才触发重新注册。
                        let _: () = msg_send![ns, setCollectionBehavior: 272u64];
                        let _: () = msg_send![ns, setIgnoresMouseEvents: true];
                        let _: () = msg_send![ns, orderFrontRegardless];
                    }
                    let window_for_reassert = window_clone.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        let _ = app_for_reassert.run_on_main_thread(move || {
                            let Ok(handle) = window_for_reassert.ns_window() else {
                                return;
                            };
                            let ns = handle as *mut AnyObject;
                            if ns.is_null() {
                                return;
                            }
                            unsafe {
                                let _: () = msg_send![ns, setCollectionBehavior: 273u64];
                            }
                        });
                    });
                }
            }
            Err(_) => {
                let _ = window_clone.show();
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_less_computer_glow<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 隐藏全屏彩虹描边浮层。
#[cfg(target_os = "macos")]
pub(crate) fn hide_less_computer_glow<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("less-computer-glow") {
        // issue #470：先通知前端「不可见」卸载全屏发光层(4 条无限动画)，webview 隐藏后即零 GPU；
        // 否则 .hide() 后 webview 仍持续合成发光层（Windows 尤其不释放动画）。
        let _ = window.emit("less-computer-glow:active", false);
        let _ = window.hide();
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_less_computer_glow<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 抓完选区后把焦点重新交回 QA 浮窗（Windows focus-dance 下半场）。begin_qa_session
/// 在 capture_selection 跑完时调；非 Windows 平台是 no-op。issue #466。
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
    // 函数名沿用历史命名，实际行为已切到「show + focus」—— 让 QA webview 真正拿到键盘
    // 焦点，ESC 才能到 React 监听、X 按钮 first-click 才不会被 OS 当作激活点击吃掉。
    //
    // 走 Tauri 的 show() / set_focus() 而不是 Win32 SetForegroundWindow + SetFocus
    // 的原因（pr_agent 关注点二轮回应）：
    //   - 直接 SetFocus(host_hwnd) 不保证 WebView2 child 收键盘事件，WebView2 子窗口
    //     有自己的 focus 模型。Tauri 内部走 webview 专用路径，能把焦点真正送到 webview。
    //   - SetForegroundWindow 在 Win11 focus-stealing prevention 下可能被拒。Tauri
    //     2.x 在跨平台 abstraction 里做了兜底（按 SPI 临时调整 / attach input queue）。
    //
    // 对 issue #164 "QA 浮窗不抢前台 app 焦点"的取舍：浮窗出现时会短暂成为前台，
    // 但 begin_qa_session 抓选区前 focus-dance 会把焦点临时还给用户原 app（见
    // coordinator.rs 同 issue 注释），抓完再 refocus_qa_window 收回 —— 选区路径
    // 仍能正常工作，issue #164 在「QA 出现的那一帧」短暂被违背是 #466 修复的代价。
    if window.show().is_err() {
        return false;
    }
    let _ = window.set_focus();
    true
}

/// 输入目标显示器的物理矩形（虚拟桌面坐标）+ DPI 缩放。
#[cfg(target_os = "windows")]
pub(crate) struct ForegroundMonitor {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
    /// 工作区矩形（physical px，去掉任务栏）。多端一致：胶囊优先夹到工作区内，
    /// 避免压住任务栏。取不到时回退为整屏矩形。issue #470。
    pub(crate) work_left: i32,
    pub(crate) work_top: i32,
    pub(crate) work_right: i32,
    pub(crate) work_bottom: i32,
    /// 该显示器的有效 DPI 缩放（1.0 = 96dpi）。
    pub(crate) scale: f64,
}

/// 用 Win32 定位「当前前台窗口（= 用户正在输入的 App）」所在的显示器。
/// 多显示器下用它把胶囊摆到「正在输入的那块屏」。`window.current_monitor()`
/// 返回的是胶囊窗口自己所在的显示器，因此不能用它来跟随输入位置。
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
        // 取不到时退回 96dpi 继续，不让定位整体失败。
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

/// 把 capsule 窗口移到屏幕底部居中，与 Swift `CapsuleWindowController.repositionToBottomCenter` 同效。
/// 留 80pt 给 macOS Dock；Windows 任务栏一般在底部 48pt 以内，整体也合适。
pub(crate) fn position_capsule_bottom_center<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    translation_active: bool,
) -> tauri::Result<()> {
    let bounds = capsule_window_bounds(translation_active);

    // Windows：跟随「正在输入的 App」所在显示器摆放，避免多显示器下胶囊
    // 总是固定出现在主屏 / 胶囊自己那块屏。
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

            let mon_w = mon.right - mon.left;
            let x = mon.left + ((mon_w - phys_w) / 2).max(0);
            // 与既有行为一致：「距底部 visual高度 + 80 + inset」，按 physical px 计算。
            let offset_from_bottom =
                (capsule_visual_height(translation_active) + 80.0 + bounds.bottom_inset) * scale;
            let y = ((mon.bottom as f64) - offset_from_bottom).round() as i32;

            // #470：四边都夹到「工作区」内（去掉任务栏），保证整窗可见。GetMonitorInfoW
            // 取不到 rcWork 时（理论上不会，rcWork 总随 rcMonitor 一同填）退回整屏矩形。
            let (work_l, work_t, work_r, work_b) =
                if mon.work_right > mon.work_left && mon.work_bottom > mon.work_top {
                    (mon.work_left, mon.work_top, mon.work_right, mon.work_bottom)
                } else {
                    (mon.left, mon.top, mon.right, mon.bottom)
                };
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
        // 仅当 Win32 取不到前台显示器时，落回下面的 current_monitor 逻辑。
    }

    // macOS：跟随鼠标光标所在显示器，而不是胶囊窗口上一次停留的显示器。
    // 这样在任意外接屏 / 任意 Space 上触发时，隐藏态胶囊也能先移动再出现。
    #[cfg(target_os = "macos")]
    {
        if let Some(mon) = capsule_target_monitor(window) {
            window.set_size(LogicalSize::new(bounds.width, bounds.height))?;
            let frame = mon.logical_frame();
            let (x, y) = bottom_visual_position(
                frame,
                bounds.width,
                capsule_visual_height(translation_active),
                80.0,
                bounds.bottom_inset,
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
    let size = monitor.size();
    let pos = monitor.position();
    let frame = logical_monitor_frame(pos.x, pos.y, size.width, size.height, scale);
    let (x, y) = bottom_visual_position(
        frame,
        bounds.width,
        capsule_visual_height(translation_active),
        80.0,
        bounds.bottom_inset,
    );
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
    // 纯光效语音舞台（siri-glsl 原始比例）：光条横贯 ~420px + 发光扩散余量。
    // 与前端 src/lib/capsuleLayout.ts 的 VOICE_ORB_STAGE_* 保持一致。
    let _ = translation_active;
    CapsuleWindowBounds {
        width: 460.0,
        height: 180.0,
        bottom_inset: 0.0,
    }
}

fn capsule_visual_height(_translation_active: bool) -> f64 {
    // 故意小于窗口高(180)：窗口整体下沉 40px，让光条视觉中心落回原胶囊的高度
    // 附近（bottom_visual_position 以此值算窗口顶 y = 屏底 - 80 - visual_height）。
    // 底部 40px 只是发光余量，窗口透明 + 鼠标穿透，压到 Dock 上方无碍。
    140.0
}

fn capsule_height_for_qa() -> f64 {
    capsule_visual_height(false)
}

#[cfg(test)]
mod tests {
    use super::{
        bottom_center_position, bottom_visual_position, capsule_height_for_qa,
        capsule_visual_height, capsule_window_bounds, clamp_to_monitor, frame_contains_point,
        frame_distance_to_point_squared, logical_monitor_frame, parse_tray_style_pack_menu_id,
        resolve_tray_style_pack_id, rotate_log_if_too_large, tray_style_menu_enabled,
        tray_style_pack_menu_entries, tray_style_pack_menu_id, LogicalMonitorFrame, TrayLabels,
        LOG_ROTATE_LIMIT_BYTES,
    };
    use crate::types::{builtin_style_pack_for_mode, PolishMode, StylePack, StylePackKind};
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
    fn capsule_window_bounds_stay_fixed_for_translation_badge() {
        let bounds = capsule_window_bounds(true);
        assert_eq!(
            (bounds.width, bounds.height, bounds.bottom_inset),
            (460.0, 180.0, 0.0)
        );
    }

    #[test]
    fn capsule_visual_height_sinks_stage_toward_dock() {
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

    #[test]
    fn bottom_visual_position_keeps_capsule_on_upper_monitor() {
        let frame = LogicalMonitorFrame {
            x: 0.0,
            y: -900.0,
            width: 1440.0,
            height: 900.0,
        };

        let pos = bottom_visual_position(frame, 220.0, 96.0, 80.0, 0.0);

        assert_eq!(pos, (610.0, -176.0));
    }

    // ---- #470: capsule 四边 clamp（纯函数，合成多显示器 / 负原点 / 1.5x DPI 输入）----

    #[test]
    fn clamp_to_monitor_leaves_on_screen_position_untouched() {
        // 1080p 主屏正中偏下，整窗本就可见 → 原样返回。
        let (x, y) = clamp_to_monitor(800, 900, 264, 126, 0, 0, 1920, 1040);
        assert_eq!((x, y), (800, 900));
    }

    #[test]
    fn clamp_to_monitor_pulls_back_off_screen_right_and_bottom() {
        // x/y 算到了屏幕右下外侧 → 收回到「右下角减去窗口尺寸」，整窗仍可见。
        let (x, y) = clamp_to_monitor(2000, 1200, 264, 126, 0, 0, 1920, 1040);
        assert_eq!((x, y), (1920 - 264, 1040 - 126));
        // 整窗右/下边界都落在 area 内。
        assert!(x + 264 <= 1920);
        assert!(y + 126 <= 1040);
    }

    #[test]
    fn clamp_to_monitor_pushes_into_negative_origin_left_monitor() {
        // 副屏在主屏左侧（负 X 原点），落点算到了副屏左外侧 → 夹回 area_left。
        // 1.5x DPI 下尺寸偏大，但 area 仍宽于窗口，左上角夹到 (-2560, top)。
        let (x, y) = clamp_to_monitor(-3000, -100, 294, 138, -2560, 0, 0, 1440);
        assert_eq!(x, -2560);
        assert_eq!(y, 0);
        // 右/下仍在 area 内。
        assert!(x >= -2560 && x + 294 <= 0);
        assert!(y >= 0 && y + 138 <= 1440);
    }

    #[test]
    fn clamp_to_monitor_respects_work_area_above_taskbar() {
        // 工作区底部 = 1040（任务栏占了 1040..1080）。落点本在任务栏区域（y=1030），
        // 应被夹到「工作区底 - 窗口高」之上，胶囊整窗不压任务栏。
        let (_x, y) = clamp_to_monitor(800, 1030, 264, 126, 0, 0, 1920, 1040);
        assert_eq!(y, 1040 - 126);
        assert!(y + 126 <= 1040);
    }

    #[test]
    fn clamp_to_monitor_degrades_gracefully_when_window_wider_than_area() {
        // 病态输入：area 比窗口还窄（罕见，但要保证不 panic、不溢出为负超界）。
        // max_x 钳到 area_left，clamp 把左上角收回 area_left。
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
