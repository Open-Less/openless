# 03：全局热键与窗口

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。对应缺口：L01、L10。系统注册和窗口效果由 Linux Host 负责，业务动作继续调用 Core。

## 1. 已有实现

- **fcitx5 热键监听**：`hotkeys.rs` `Fcitx5HotkeyListener::start()/drain()/take_error()`——经 fcitx5 插件路由热键事件（已实现待实测，见 L11）。
- **fcitx5 输入/选区**：`fcitx5.rs`——插件安装（AppImage 在 listener 前安装）、PRIMARY 选区、落字。
- **单实例**：`single_instance.rs` `SingleInstanceGuard::acquire(path)` + `SingleInstanceBroker::acquire_or_forward()/drain()`——二次启动意图转发给首实例。
- **设置侧拒绝逻辑**：`settings.rs:56-61` 对 `switch_style` / `open_app` / `style_packs` 三类热键修改给出明确拒绝信息（L01 的直接证据）。
- **能力探测**：`capabilities.rs` `LinuxCapabilitySnapshot`（session 类型、`PlatformCapabilities`、权限快照），`supports_tray` 只是探测字段。

## 2. 缺口（L01）

全局热键修改（改绑 `switch_style`、`open_app`、`style_packs`）被 settings 拒绝，因为 Host 尚未提供真实的系统级注册/重绑/失败恢复/重启还原。关闭标准：实际注册、触发、重绑、失败恢复与重启还原，分别声明 X11/Wayland 支持范围。

## 3. 缺口（L10，窗口侧）

托盘、窗口/后台运行策略、通知、自启、更新器均未实现（capabilities 仅探测）。参考[05](05-native-host-and-data.md)第 4 节。
