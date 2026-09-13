# 04：页面与领域操作

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。页面定义：`linux-egui/src/ui_state.rs` `Page` 枚举（10 页）；主循环与交互：`main.rs`。

## 1. 十页现状

| 页面 | 已有操作 | 主要缺口 |
| --- | --- | --- |
| Start | 下一步引导、环境准备说明（fcitx/插件、桌面会话、麦克风、Secret Service 的真实可知状态；未连接时保留诊断步骤，不展示成功） | — |
| Dictation | 普通听写（复用 Core `dictation_engine`）、ticket 化落字 | — |
| Qa | QA 文本/语音（`qa.rs`）、全局待办、审批、取消 | 编辑模式/应用/撤回 UI（L04） |
| Selection | 划词润色预览/撤回（`selection.rs`） | Selection Voice 生产触发/捕获/意图路由（L04） |
| Agent | Less Computer 工具调用、审批固定在页面外、后台事件保留 | Agent 检测/模型/路径/权限配置（L09） |
| Services | AI 服务配置、渠道管理（`settings.rs` + Core provider 面） | Omni 有效模式配置入口完整性（L09） |
| Models | 本地 Qwen 下载/激活/取消 | 完整模型管理：路径/镜像/详情/删除/预载/释放（L08） |
| Remote | 手机输入独立页、连接状态、陈旧地址处理 | — |
| History | 只读最近条目 | 完整历史/统计/录音操作（L07，依赖 L03） |
| Settings | 现有设置独立页 | 词典/纠错/风格包/市场无页面（L05/L06）；多数设置缺实际消费者（L09） |

## 2. 跨页约定

- Agent 审批固定在页面外展示，QA/Selection/Agent 的后台事件保留操作或导航提示。
- 事件消费不得因导航停止（见[06](06-events-and-sessions.md)）；会话取消语义由 Core 承担。
- 每个旧操作保留真实调用；页面增删不改变 Core 接口。

## 3. 新增页面的实施要求

L04–L08 每个领域补完整成功/失败/取消路径后再算完成；领域功能扩展时优先拆分 `main.rs`（当前较大），避免继续单文件增长。
