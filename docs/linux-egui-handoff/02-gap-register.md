# 02：Linux 缺口登记与实施顺序

状态：以当前 `openless-linux-egui` 源码盘点；更新：2026-09-24。此表区分代码实现、自动验证和设备验证，不把待实测项目记作未实现。

## 1. 状态定义

- **待实现**：Linux 生产路径缺少实际代码或用户入口。
- **已实现，待设备验证**：生产代码和自动化覆盖已存在，仍需真实桌面、设备或安装证据。
- **明确不支持**：当前 Linux 能力边界，不得在 capability/UI 中宣称可用。

## 2. 待办登记

| ID | 当前状态与源码锚点 | 关闭标准 |
| --- | --- | --- |
| L01 | **部分完成**：QA 编辑预览的“确认并替换”现已按 Tauri 处理应用失败/成功/取消结果，失败时保留预览；Selection Voice 本身仍无 Linux 生产触发和录音/意图/应用完整编排 | 从按键边沿、捕获原选区、录音终结、意图分流到 QA/编辑预览及应用/取消/撤回均按 session 所有权执行；失败/迟到结果不能作用于新会话 |
| L02 | **明确不支持**：Linux 不保证前台应用身份或插入后手改观察；`backend.rs` 使用 Core Noop HostContext/EditObservation adapter | 保持 `source_app = None` 和 capability 真实；只有找到跨 X11/Wayland 可靠且尊重隐私的机制才重开 |
| L03 | **已实现，待设备验证**：`audio.rs` CPAL 录音、WAV 归档、保留策略、静音/恢复、提示音及历史录音操作已接入 | X11/Wayland 验证设备拔插、取消/错误/退出后的恢复、提示音与实际音量设备行为 |
| L04 | **已实现，待设备验证**：设置热键使用 fcitx5 插件，不提供 IBus/系统全局热键替代 | X11/Wayland 验证触发、冲突拒绝、重绑/重启恢复及不支持场景提示 |
| L05 | **已实现，待设备验证**：远程手机输入 HTTPS/WSS、TLS 身份、PIN、录音页、Core 外部音频会话与设置 UI 位于 `remote_input.rs` / `runtime.rs` / `settings.rs` | 用真实手机和 Linux 主机完成同网连接、证书信任、PIN、录音插入/仅转写、断线恢复、禁用/关停撤销；记录平台/浏览器矩阵 |
| L06 | **已实现，待设备验证**：Secret Service 凭据、Qwen 本地推理、fcitx5、托盘/通知/自启及更新器均有 Host 实现 | 逐项在目标发行版/桌面运行验证；Qwen 记录真实推理，更新器记录签名安装/回滚 |
| L07 | **明确不支持**：Linux 不承诺 IBus 或通用全局热键；Selection Voice 在 L01 完成前保持隐藏 | 能力发现、设置和产品文案持续反映实际能力 |
| L08 | **已实现，待设备验证**：deb/rpm/AppImage 包和发布工作流已存在 | 完成安装、升级、回滚、签名与分发验收记录 |

## 3. 实施顺序

1. 完成 L01，按 Tauri 的 Selection Voice 状态机接线，并保留 Linux 对不可用目标能力的显式拒绝。
2. 完成 L05 的真实手机端到端验证；自动化测试通过不能替代手机/桌面证据。
3. 依序补齐 L03/L04/L06/L08 的设备证据；L02/L07 是明确边界，不伪造支持。

## 4. 源码审计补充

- `openless-core` 内有 **5 处 `target_os = "linux"` 条件编译，分布在 4 个文件**：ASR frame、Volcengine ASR、`shared_types.rs` 两处、polish。另有 2 处 `cfg(unix)`，分别是事务替换和录音归档，属于 Unix 通用逻辑。其余 Linux 文本主要是文档/测试或平台中立策略说明。
- 目前不建议拆 Core：这些条件编译只覆盖少量输入格式/供应商差异，服务、策略和契约仍平台中立；Linux 的 DBus、录音、fcitx5、桌面窗口和远程输入适配器已经位于 `linux-egui`。
- 无边框窗口拖动的触摸屏限制来自 Wayland 拖动协议所需的指针按键 serial，winit 丢弃触摸 serial；升级 egui 不能改变该协议条件。桌面侧可用 KWin 的 **Alt+F7** 移动窗口（键盘移动后用触摸板/方向键定位）；若当前会话是 X11，可由窗口管理器移动。此项目不在应用内伪造拖动实现。
- 依赖升级与 Vulkan 后端切换已落入源码；见 [`linux-egui-dependency-upgrade.md`](../linux-egui-dependency-upgrade.md)。设备图形路径仍需在真实 GPU/Wayland/X11 主机验收。

记录格式：`ID / commit / 实现效果 / 自动证据 / 设备证据 / 剩余限制`。
