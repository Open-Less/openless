# 03：全局热键与窗口

对应缺口：L01、L10。系统注册和窗口效果由 egui 团队的 Linux Host 负责，业务动作继续调用 Core。

## 1. 历史与当前差异

Tauri 1.x 在 **X11** 上已有切换风格、打开主窗和风格包热键，经过 `global-hotkey` 路径。
源码锚点为 `v1.3.18-tauri` 的 `src-tauri/src/lib.rs` 三个 `start_*_hotkey_listener` 和 `coordinator/hotkey_loops.rs` 的实际处理分支。
这不能证明原生 Wayland/fcitx 路径曾具备同等能力，也不能写成“1.x 从未实现”。

当前 Linux 已用 fcitx5 接听写、QA、选区润色、翻译和 Less Computer；但以下三项仍缺 Host 接线：

| Core配置目标 | 当前缺口 | 触发后的业务入口 |
| --- | --- | --- |
| `HotkeyRuntimeTarget.switch_style` | Linux settings 拒绝修改，没有对应 native event 分支 | `activate_previous_style_pack` |
| `HotkeyRuntimeTarget.open_app` | 同上 | `request_host_action(HostAction::ShowMain)` |
| `HotkeyRuntimeTarget.style_packs` | 同上 | `activate_style_pack`，使用绑定的稳定风格包ID |

入口见[Core api.rs](../../openless-all/app/crates/openless-core/src/api.rs)与[HostAction](../../openless-all/app/crates/openless-core/src/ports.rs)。

## 2. 应修改的生产路径

- [settings.rs](../../openless-all/app/linux-egui/src/settings.rs)：`reject_unsupported_hotkey_changes`、`LinuxSettingsEffects::apply_hotkeys` 和 prepare/commit/restore。
- [lib.rs](../../openless-all/app/linux-egui/src/lib.rs)：`LinuxHotkeyEvent` 消费及业务派发。
- [runtime.rs](../../openless-all/app/linux-egui/src/runtime.rs)：启动同步、listener 重连及 pump。
- [fcitx5.rs](../../openless-all/app/linux-egui/src/fcitx5.rs)与[插件](../../openless-all/scripts/linux-fcitx5-plugin)：按所选原生机制扩展注册和事件传输。
- [main.rs](../../openless-all/app/linux-egui/src/main.rs)：快捷键编辑、冲突/不可用提示及窗口消费。

不能仅删除 `Unsupported` 判断：只有原生注册和实际事件路径一起完成，配置才算生效。

## 3. 注册事务

1. UI 提交草稿和读取时的 preferences revision，经 LinuxHost 调 Core 设置验证。
2. Host 按 effect plan 注册新目标、清除旧目标；资源占用/无权限必须返回错误。
3. 部分失败按 receipt 恢复旧注册及配置；不能保存“新键”却仍监听“旧键”。
4. 启动和 fcitx5 重连读取同一 Core target 重新同步；退出/禁用时释放注册。
5. 听写的 Hold/Toggle/Auto、去抖/冷却仍归 Core；Host 提供真实边沿，不另建解释器。

X11 与 Wayland/桌面环境分别报告支持情况。窗口内的 egui 键盘事件不等于系统全局热键；焦点离开 OpenLess 后仍须按承诺范围工作。

## 4. 窗口和后台行为

现有 `ShowMain`/`FocusMain` 已映射 egui viewport，单实例有转发机制；保留这些实现。
尚需完整托盘/后台窗口行为、录音反馈层、桌面通知与真正重启。`ShowDictationFeedback`/`HideDictationFeedback` 当前不产生效果，通知只进状态栏。

窗口聚焦不得覆盖录音开始时捕获的输入目标。关闭业务面板应取消其会话；隐藏主窗、退出进程、关闭面板是不同操作。

## 5. 关闭证据

逐项覆盖：首次保存、重绑、旧键失效、冲突和回滚、重启还原、插件重载、后台触发、禁用/退出释放；风格包删除/禁用后的绑定也需一致。
测试结果分别标注 X11/Wayland、桌面环境、实际前台应用。目标环境无法提供某动作时明确禁用并说明限制，不返回假成功。
