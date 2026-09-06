# Core / Host 平台边界审计

> 2026-09-06范围更新：[当前2.0需求](./2.0-requirements.md)要求Windows/macOS完整保留各自1.x功能；Linux剩余Host/UI和产品验收移交egui团队，见[拆分交接目录](./linux-egui-handoff/README.md)。本审计保留代码边界证据，不表示Linux产品完整；Linux应用待办不单独阻塞本批桌面2.0。

审计基线：OpenLess 2.0.0-Beta.1（2026-09-04 工作树）。范围覆盖 `openless-core`、Tauri、Linux egui、React/TypeScript、C++ 插件及构建脚本；vendor、生成物和纯视觉实现排除。

2026-09-05 更新：下文为边界迁移索引；最新生产行为缺口、修复和证据在
[`pr1019-2.0-final-review.md`](./pr1019-2.0-final-review.md)。有接口和通过 fixture
不能证明对应 Host 的启用、取消、资源释放或实际输入行为正确。

## 已迁移项

- Provider descriptor、Credential channel mutation/active/order、ASR/LLM/Omni、设置事务与事件位于 Core；React/egui 只渲染 descriptor 和本地化标签。
- `LessComputerVoiceSession` 在 Core 持有 session lease、ASR snapshot、PCM 校验、TranscriptDelta、静音/fault 决策和 Agent submit。
- 主听写与 Selection Voice 的 Hold/Toggle/Auto/Combined、press generation、250ms debounce、450ms terminal cooldown 均由 Core 解释。
- 录音 plan、`SilenceAutoStop`、silent retry、correction 顺序/归因、流式 final reconciliation、edit observation generation 与历史重转 mutation 均位于 Core。
- `ModelStore` 统一 1.x/custom-root 迁移与 Local ASR 原子激活；Qwen/Whisper 用 target generation lease，Linux Qwen 使用打包 runtime 和进程组 cancel/timeout。

## 本批修复项

- Shared realtime ASR 将 Qwen、StepFun、Bailian、Volcengine、讯飞 interim 回调接入 Unicode replace-from `TranscriptDelta`；React/Linux 使用同一 reducer 语义。
- Linux production factory 注入 QA、Remote Input、Selection preview/revert、Provider/Model、Less Computer 与打包 Qwen runtime；AppImage 在 hotkey listener 前处理 fcitx5 插件。
- Tauri 只保留窗口、原生录音/native ASR、秘密存储、插入目标、进程和协议 transport effect；Credential/QA/Selection/stream/history 的旧 Host policy 已删除并由 source gate 固定。
- Linux/Tauri manifest、打包脚本和 AppStream 使用 `AGPL-3.0-only`；2.0.0-Beta.1 为许可证生效边界，1.x 发布物仍为 MIT。

## 有意保留的 Host 项

- Tauri 窗口、胶囊、原生录音/native ASR、系统凭据、插入和生命周期。
- Linux cpal、fcitx5、Secret Service、资源布局和单实例。
- Windows Foundry/Sherpa、macOS Apple Speech/MLX/Whisper 等单平台 runtime。

## Deferred 候选

- 不再为当前迁移拆独立 process/transport crate；现有 Core `AgentCommand` + Host `ProcessAdapter` 已满足边界，新增 crate 只会制造中间层。
- Ubuntu 实际焦点输入、音频设备、Secret Service、QA/Remote/Selection、Qwen 性能与 deb/rpm/AppImage 安装升级回滚证据。
- Windows 原目标恢复/PasteSent/TSF unknown/mute/fault/Foundry/Sherpa，macOS newline/context/edit/login-shell，以及 Android readiness/overlay/IME 的真实设备 smoke。

## Standards

依赖方向保持 Host → Core；Core 不引用 Tauri/egui，凭据只经 `CredentialStore`，未注入 runtime 明确返回 `Unsupported`。共享 session 通过一个 mutable lease 和 `BackendEvent.session_id` 关联。

## Spec

2.0 目标的 Core voice session、实时增量、三档热键解释和 Linux typed event 已有公共入口；平台原生录音/本地模型仍由 Adapter 提供，不能以 headless fixture 代替设备证据。

## 剩余风险和证据边界

本审计证明源码边界和可运行 contract，不证明云服务凭据、真实音频设备、CLI 安装、签名、设备运行或发布产物。Backend contract 版本为 `2.0.0`，不随应用版本升级。
