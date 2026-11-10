# Linux Web 宿主：命令面移植清单

`linux-web`（GTK + WebKitGTK 跑官方 React）里唯一的手工活，是把前端
`invokeOrMock('...')` 用到的命令接到 `openless-core`。这份文档记录**清点结果**
和**移植模板**，让这一步变成机械工作。

## 清点结果（自动统计，可重跑）

```
前端命令 184  ·  Tauri 命令实现 267  ·  已覆盖 180  ·  缺 4
```

- **180 条**在 `src-tauri/src/commands/*.rs` 里已有实现 → 照抄函数体即可，
  它们绝大多数只是 `core.xxx()` 的一行封装。
- **4 条没有 Tauri 实现**，都是平台/生命周期相关，Linux 侧按需给空实现或真实现：
  - `is_wayland_cli_mode`
  - `log_client_error`
  - `restart_app`
  - `reset_accessibility_permission_and_restart_app`

重跑统计：

```bash
cd openless-all/app && python3 - <<'PY'
import re, pathlib
cmds = set()
for f in pathlib.Path('src/lib/ipc').glob('*.ts'):
    cmds |= set(re.findall(r"invokeOrMock(?:<[^>]*>)?\(\s*['\"]([a-zA-Z0-9_]+)['\"]", f.read_text()))
tauri = set()
for f in pathlib.Path('src-tauri/src/commands').glob('*.rs'):
    t = f.read_text()
    tauri |= set(re.findall(
        r"#\[tauri::command\][^\n]*\n(?:#\[[^\]]*\]\n)*pub (?:async )?fn ([a-z0-9_]+)", t))
print(len(cmds), len(tauri), len(cmds & tauri), sorted(cmds - tauri))
PY
```

## 移植模板

Tauri 侧（`commands/*.rs`）：

```rust
#[tauri::command]
pub async fn list_style_packs(core: CoreState<'_>) -> Result<Vec<StylePack>, String> {
    core.list_style_packs().await.map_err(|e| e.to_string())
}
```

`linux-web` 侧（`src/main.rs::dispatch`）：

```rust
"list_style_packs" => {
    let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
    let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
    let packs = runtime
        .block_on(backend.list_style_packs())
        .map_err(|error| error.to_string())?;
    serde_json::to_value(packs).map_err(|error| error.to_string())
}
```

三条约定：

1. **同步方法**用 `runtime.block_on(...)`；已经在 `dispatch` 里同步执行，不走 async fn。
2. **参数按 camelCase 取**（前端传的是 camelCase），必要时同时兼容 snake_case。
3. **错误只传 message**，除非 Tauri 侧有特殊约定（例如
   `validate_provider_credentials` 的 `siwc:` 前缀）。

## 已完成（15 条）

启动 / 设置：`get_startup_snapshot` `get_settings` `get_settings_snapshot`
`set_settings` `update_setting_fields`
凭据：`get_credentials` `read_credential` `set_credential`
`validate_provider_credentials`
渠道：`list_channels` `create_channel` `delete_channel` `set_channel_enabled`
概览：`list_history` `get_activity_stats` `list_provider_descriptors`

## 建议的移植顺序

1. **词库/纠错**：`list_vocab` `add_vocab` `remove_vocab` `update_vocab`
   `list_vocab_presets` `save_vocab_presets` `list_correction_rules`
   `add_correction_rule` `remove_correction_rule` `set_correction_rule_enabled`
   `accept_pending_correction` `reject_pending_correction` `dismiss_vocab_suggestions`
2. **风格包**：`list_style_packs` `save_style_pack` `delete_style_pack`
   `set_active_style_pack` `set_style_pack_enabled` `set_style_pack_hotkeys`
   `preview_style_pack_runtime` `import_style_pack_from_zip` `export_style_pack_to_zip`
   `get_default_style_system_prompts` `reset_builtin_style_pack`
3. **听写/录音**：`start_dictation` `stop_dictation` `cancel_dictation`
   `list_microphone_devices` `start_microphone_level_monitor`
   `record_channel_test` `read_audio_recording` `retranscribe_recording`
4. **热键**：`set_dictation_hotkey` `set_qa_hotkey` `set_quick_note_hotkey`
   `set_selection_polish_hotkey` `set_translation_hotkey` `set_switch_style_hotkey`
   `set_open_app_hotkey` `set_combo_hotkey` `validate_*hotkey*` `get_hotkey_status`
5. **云同步**：`cloud_sync_e2ee_*`（20 条）+ `cloud_sync_status` / `upload` / `restore`
6. **更新/杂项**：`check_network` `app_check_update_with_channel`
   `get_update_channel` `set_update_channel` `export_error_log` `get_service_usage`
7. **最后**：4 条无 Tauri 实现的命令（Linux 侧 stub 或真实现）。

## 云同步这一组的注意点（实测，2026-11）

`cloud_sync_e2ee_*` 的 core API **同步/异步混用**：

| 形态 | 例子 |
|---|---|
| 同步 `Result<T>` | `cloud_sync_e2ee_status()` |
| 异步 `.await` | `cloud_sync_e2ee_lock()`、`cloud_sync_e2ee_sync_now()`、`cloud_sync_*` |

而且返回类型各不相同（`EncryptedSyncStatus`、`String`、`()`、快照结构…），
`dispatch` 的每个分支必须自己收敛成 `Result<Value, String>`。建议：

```rust
fn json_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}
// 分支统一写成： Ok(json_value(expr))
```

不要用 `json_value(..)?` 再 `?` 一次——那会把 `Value` 再套一层 `Result`，
`match` 分支类型就不一致了（这一步踩过，已回退）。
