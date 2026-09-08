# Linux egui 接入交接

更新：2026-09-06；接口以本目录所在提交的代码为准。初始缺口盘点基于`2b315572`，后续资源生命周期与VoiceState合同更新见01/06。需求依据：[2.0范围](../2.0-requirements.md)。

本批2.0完整交付Windows/macOS。Linux交付共享Core及接入资料；egui团队负责余下Linux Host、UI、集成和Linux发布验收。
已有`linux-egui`代码和包构建是可复用起点，不等于Linux产品已完成。

## 阅读顺序

| 文档 | 回答什么问题 |
| --- | --- |
| [01 Core接入合同](./01-core-contract.md) | 从哪里接、哪些规则归Core、生命周期和秘密边界 |
| [02 缺口与实施顺序](./02-gap-register.md) | 当前Linux还缺什么、由谁做、依赖顺序和关闭标准 |
| [03 全局热键与窗口](./03-hotkeys-and-windows.md) | 三类1.x X11热键缺口及原生注册、恢复和窗口效果 |
| [04 页面与领域操作](./04-ui-domains.md) | 已有页面能做什么、哪些Core能力尚缺操作入口 |
| [05 原生宿主与数据](./05-native-host-and-data.md) | 音频/插入/凭据/模型/进程/托盘/更新的接入与验证 |
| [06 事件与会话](./06-events-and-sessions.md) | sequence、重放、Unicode增量、QA/Agent和取消时序 |
| [07 验收与证据](./07-acceptance.md) | 自动命令、设备矩阵、Linux发布条件和回报模板 |

## 工作规则

- 先复用现有Core和Linux Adapter；不复制Tauri业务、不让UI读取秘密或私有Coordinator状态。
- Linux系统功能由团队实现Host Adapter，egui框架只提供UI能力，不会自动注册全局热键或管理系统服务。
- Core接口不足：带最小复现、所需业务语义和期望事件回报Core负责人；不要在UI临时补另一套规则。
- 文档中的“已有”仅表示本快照有相应代码/合同；设备与发布证据单独确认。
- 任务关闭时更新[缺口登记](./02-gap-register.md)及对应小文档，附准确commit、测试和设备结果。

## 入口文件

- [Core公开出口](../../openless-all/app/crates/openless-core/src/lib.rs)
- [Linux生产factory](../../openless-all/app/linux-egui/src/backend.rs)
- [LinuxHost facade](../../openless-all/app/linux-egui/src/lib.rs)
- [现有egui主循环/UI](../../openless-all/app/linux-egui/src/main.rs)
- [headless示例](../../openless-all/app/linux-egui/examples/headless_host.rs)
- [历史修复证据](../pr1019-2.0-final-review.md)

旧的[长接口文档](../linux-egui-backend-contract.md)与[阶段计划](../linux-egui-shared-backend-plan.md)保留历史参考；范围冲突以新需求和本目录为准。
