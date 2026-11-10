# linux-web 命令缺口（自动生成，可重跑）

前端 `invokeOrMock` 共 223 条；`linux-web` 已实现 82 条；缺 141 条。

## 移动端 / 平台专属（Linux 不适用）（63）

check_accessibility_permission check_microphone_permission experimental_asr_acknowledge experimental_asr_delete experimental_asr_set_enabled experimental_asr_status foundry_local_asr_cancel_prepare foundry_local_asr_catalog foundry_local_asr_delete_model foundry_local_asr_download foundry_local_asr_model_dir foundry_local_asr_prepare foundry_local_asr_release foundry_local_asr_reveal_model_dir foundry_local_asr_set_keep_loaded_secs foundry_local_asr_set_language_hint foundry_local_asr_set_model foundry_local_asr_set_runtime_source foundry_local_asr_status get_windows_ime_status handle_window_hotkey_event local_asr_activate local_asr_cancel_download local_asr_cleanup_incomplete local_asr_delete_model local_asr_download_model local_asr_engine_status local_asr_fetch_hf_card local_asr_fetch_remote_info local_asr_get_settings local_asr_list_models local_asr_model_dir local_asr_preload local_asr_release_engine local_asr_reveal_model_dir local_asr_reveal_models_root local_asr_set_active_model local_asr_set_keep_loaded_secs local_asr_set_mirror local_asr_set_models_base_dir local_asr_storage_settings local_asr_test_channel local_asr_test_model open_android_local_migration request_accessibility_permission request_microphone_permission sherpa_onnx_asr_cancel_download sherpa_onnx_asr_cancel_prepare sherpa_onnx_asr_catalog sherpa_onnx_asr_delete_model sherpa_onnx_asr_download_model sherpa_onnx_asr_fetch_remote_info sherpa_onnx_asr_model_dir sherpa_onnx_asr_prepare sherpa_onnx_asr_release sherpa_onnx_asr_reveal_model_dir sherpa_onnx_asr_set_language_hint sherpa_onnx_asr_set_model sherpa_onnx_asr_status siwc_action siwc_models siwc_status trigger_microphone_prompt

## 仓库 / 市场（Linux 未开）（6）

marketplace_detail marketplace_download marketplace_install marketplace_like marketplace_list marketplace_upload

## 语音编辑 / 选区（Linux 走 fcitx5 另一条链）（20）

apply_quick_note_repolish cancel_selection_polish_preview cancel_selection_voice_intent_prompt cancel_voice_edit_session commit_voice_edit confirm_selection_polish_preview confirm_selection_voice_intent_prompt confirm_selection_voice_preview finalize_voice_edit_dictation get_selection_polish_preview get_selection_voice_intent_prompt get_selection_voice_preview get_voice_edit_state repolish revert_selection_voice_preview start_voice_edit_instruction start_voice_edit_session stop_voice_edit_instruction voice_edit_window_close voice_edit_window_open

## 云同步 / 更新 / 远端（9）

app_check_update_with_channel app_download_and_install_android_update cloud_sync_upload fetch_latest_beta_release get_remote_input_status get_update_channel regenerate_remote_pin set_remote_locale set_update_channel

## 频道 / 供应商 / 设置变更（纯搬，优先）（25）

add_learned_vocab clear_expression_profile clear_history create_style_pack_from_template delete_channel_if_blank delete_history_entry get_expression_profile get_hotkey_capability get_hotkey_status get_qa_hotkey_label list_provider_models open_system_settings rename_channel reorder_channels set_active_asr_provider set_active_llm_provider set_active_omni_provider set_channel_provider_type set_default_polish_mode set_desktop_onboarding_finished set_shortcut_recording_active set_style_enabled set_style_pack_icon set_vocab_enabled validate_shortcut_binding

## 胶囊 / QA 窗口 / 电平（12）

chat_panel_focus_keyboard dismiss_insert_fallback_card get_capsule_snapshot qa_get_snapshot qa_set_edit_instruction_mode qa_submit_text qa_toggle_recording qa_window_dismiss qa_window_set_expanded report_insert_fallback_card_height start_microphone_level_monitor stop_microphone_level_monitor

## 未分类（6）

get_android_audio_effects_status get_dictation_translation_target select_dictation_translation_target set_android_audio_monitoring set_capsule_language_menu set_capsule_transcript_visible

## 剩余命令的处置（2026-11 复核）

上一节的分组是自动分类，这里才是**结论**。`linux-web` 已实现 135 条，
剩余 88 条全部属于下面四类，不做不是遗漏：

### 1. 本地模型（约 55 条）—— 不做
`sherpa_onnx_asr_*`、`foundry_local_asr_*`、`local_asr_*`、`experimental_asr_*`。

Linux 版本的产品定位不含本地模型（云端 ASR/LLM 才是这条链）；接了这些命令
会得到一堆永远走不到的分支。前端在 Linux 上不会展示入口（`platformCaps`
门控），但命令本身若被调用会报「不支持」，而不是假装成功。

### 2. 平台专属（约 10 条）—— 不做
`app_download_and_install_android_update`、`set_android_audio_monitoring`、
`get_android_audio_effects_status`、`siwc_*`、`get_windows_ime_status`、
`handle_window_hotkey_event`、`check_accessibility_permission`、
`request_accessibility_permission`。

TCC 授权与 Windows IME 在 Linux 上没有对应概念；`siwc_*` 是 Windows 商店的
凭据桥。**不做假实现**：调用会返回明确的「不适用」错误。

### 3. 市场（6 条）—— 不做
`marketplace_*`。Linux 未开市场，前端入口由 `platformCaps` 关掉。

### 4. 另一条链（约 12 条）—— 需单独设计，非照搬
`voice_edit_window_*`、`start/stop_voice_edit_*`、`commit_voice_edit`、
`cancel_voice_edit_session`、`selection_voice_*`、`*_selection_polish_preview`、
`get_voice_edit_state`。

Tauri 这些靠独立窗口 + 原生输入注入；Linux 的选区/语音编辑走
fcitx5 插件（`captureSelectionTarget` / apply），窗口形态也不同。
照搬会做出两套互相打架的链路，要做得先决定 Linux 的交互形态。

## 已实现部分的对照原则

- 参数名以**前端实际调用**为准（如 `set_active_*_provider` 收 `provider` 而不是 `id`）。
- 类型以 core 为准，不猜字段：`SecretValue` 显式 `expose_secret()`、`PolishMode`
  走 serde 名字、`CloudSyncUiPreferences` / `RestoreMode` 直接反序列化。
- 数据源以**上游现状**为准：更新检查改读 CDN 清单
  （`dl.aeoform.com/updates/...`），不再读 GitHub atom。
- 没有真实语义的一律不做或明确报错，不用空实现凑数。
