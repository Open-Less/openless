# Platform adapter architecture

## Goal

把 `Coordinator` 需要的热键边沿事件（`pressed` / `released` / `cancelled`）与各平台的 OS hook 细节隔离开，避免把 UI 文案、权限判断、按键映射和 session state machine 混在一起。

## Backend boundary

Rust 层统一暴露三类对象：

- `HotkeyAdapter` trait：平台监听器只负责安装、更新 binding、发送边沿事件。
- `HotkeyCapability`：描述当前平台能提供什么（可选 trigger、是否需要辅助功能权限、是否支持 modifier-only trigger、是否有 fallback）。
- `HotkeyStatus` / `HotkeyInstallError`：描述当前 hook 是否已安装、失败原因、当前实际 adapter。

`Coordinator` 不再关心 CGEventTap / Windows hook / `rdev` 的实现差异，只消费统一事件和状态。

## Platform adapters

### macOS

- Adapter: `MacHotkeyAdapter`
- Hook: `CGEventTap`
- 目的：保留现有已验证实现，不回退到 `rdev`
- 限制：依赖辅助功能权限；授权后通常需要完全退出再重开

### Windows

- Adapter: `WindowsHotkeyAdapter`
- Hook: `SetWindowsHookExW(WH_KEYBOARD_LL)`
- 目的：支持右 Control / 右 Alt 这类 modifier-only trigger，并且保留左右侧语义
- 备注：默认推荐 `右 Control + 按住说话`

### Linux / other

- Adapter: `RdevHotkeyAdapter`
- Hook: `rdev::listen`
- 目的：best-effort 兜底，不承诺与 macOS / Windows 同等行为

## UI contract

前端通过 IPC 读取：

- `get_hotkey_capability`
- `get_hotkey_status`
- `get_settings`

设置页、权限页和快捷键提示必须基于 capability / status / actual binding 渲染，而不是再写 `if (os === 'win') ... else ...` 的平台硬编码文案。

## Explicit non-goals

- 不静默把 modifier-only trigger 替换成普通 registered shortcut
- 不把平台差异泄漏到 `Coordinator`
- 不在这层引入新的全局快捷键依赖
