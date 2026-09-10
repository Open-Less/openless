# 04：页面与领域操作

状态：canonical（2026-09-07 以源码为准重写；2026-09-10 补 2.0 界面复刻现状）；更新：2026-09-10。页面定义：`linux-egui/src/ui_state.rs` `Page` 枚举；主循环与交互：`main.rs`；2.0 视觉系统：`linux-egui/src/design_tokens.rs`（tokens.css 对照表，跨平台单测）+ `linux-egui/src/ui/`（theme/widgets/prefs，仅 Linux 编译）。

## 0. 2.0 界面复刻（2026-09-10，待真实桌面验收）

按 Tauri/React 2.0 UI 在 egui 侧 1:1 复刻视觉与结构，功能接线沿用既有 Core 调用：

- 设计令牌：`design_tokens.rs` 是 `src/styles/tokens.css` 的 Rust 对照表（浅/深双主题、圆角阶梯、侧栏/弹窗尺寸），单测锁定与 CSS 字面值一致。
- 主题：`ui/theme.rs` 把令牌转成 egui Style/Visuals（白底 zinc 体系、#2563eb 强调、控件 8px 圆角）；CJK 字体自动探测安装（含 `OPENLESS_IME_FONT` 覆盖，移植自 #997 并修正多候选覆盖问题）；深浅主题即时切换并持久化到 `$XDG_DATA_HOME/OpenLess/ui-prefs.json`（`ui/prefs.rs`，表现层状态不进 Core 合同）。
- 外壳：226px 主侧栏（版本行 + BETA 徽章 + 扁平导航 + 「工具」可展开分组 + 底部设置），对齐 `FloatingShell`；<760px 回落横向胶囊导航。
- 设置弹窗：环境与设置 / AI 服务 / 本地模型 / 手机输入四分区收纳进居中 960x680 弹窗（遮罩 + 14px 圆角 + 左 rail），对齐 `SettingsModal`；这些入口不再占主导航。
- 组件：`ui/widgets.rs`（卡片/软底卡/状态胶囊/主·蓝·危险·次级按钮/分段控件/开关/键值行/页头）；问答页按 #997 验证过的双气泡布局（用户右蓝软底、助手左 surface-2、流式/思考/错误行、底部输入组）。
- 状态：CI ubuntu 已有 linux-egui `cargo test + check` 门禁（`ci.yml` `linux-egui` 任务）；真实 X11/Wayland 桌面的视觉与交互证据仍待实测（L11）。

## 1. 页面现状

| 页面 | 位置 | 已有操作 | 主要缺口 |
| --- | --- | --- | --- |
| Start | 主导航 | 2.0 概览卡（Core/管线状态、快速开始、其他任务入口）；未连接保留诊断步骤 | — |
| Dictation | 主导航 | 普通听写（Core `dictation_engine`）、状态胶囊、转写卡 | — |
| Qa | 「工具」分组 | 2.0 双气泡会话、文本/语音提问、取消/关闭 | 编辑模式/应用/撤回 UI（L04） |
| Selection | 「工具」分组 | 划词润色预览（可编辑草稿）/确认/取消/撤回 | Selection Voice 生产触发/捕获/意图路由（L04） |
| Agent | 「工具」分组 | Less Computer 任务卡 + 输出卡；审批卡在页内 + 跨页横幅 | Agent 检测/模型/路径/权限配置（L09） |
| History | 主导航 | 只读最近 20 条卡片（时间 + 落字状态胶囊 + 文本） | 完整历史/统计/录音操作（L07，依赖 L03） |
| Services | 设置弹窗 | AI 服务状态胶囊、渠道管理（ASR/LLM 分段切换） | Omni 有效模式配置入口完整性（L09） |
| Models | 设置弹窗 | 本地模型目录卡（display name/家族/模式/语言/体积元数据）+ 下载/激活/取消 | 完整模型管理：路径/镜像/删除/预载/释放（L08） |
| Remote | 设置弹窗 | 启用/端口、连接状态、证书指纹核验、PIN/地址 | — |
| Settings | 设置弹窗 | 外观（深浅主题）、功能开关、环境准备卡 + 官方指南 | 词典/纠错/风格包/市场无页面（L05/L06）；多数设置缺实际消费者（L09） |

## 2. 跨页约定

- Agent 审批在 Agent 页内展示；其余页面顶部保留跨页审批横幅（允许/拒绝直达 Core）。
- 事件消费不得因导航停止（见[06](06-events-and-sessions.md)）；会话取消语义由 Core 承担；设置弹窗打开时 Esc 只关弹窗、不触发语音取消。
- 每个旧操作保留真实调用；页面增删不改变 Core 接口。

## 3. 新增页面的实施要求

L04–L08 每个领域补完整成功/失败/取消路径后再算完成；领域功能扩展时优先拆分 `main.rs`（当前较大），避免继续单文件增长。新增视觉元素一律引用 `design_tokens` / `ui::widgets`，不在页面里写裸色值。
