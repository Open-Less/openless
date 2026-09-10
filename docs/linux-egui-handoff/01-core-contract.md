# 01：Core 合同（Linux 可调用的面）

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。前置：[交接入口](./README.md)。本文只约束接入，不规定页面布局。

## 1. 合同文件与启动门

- 公开合同：`openless-all/app/contract/backend-2.0.json`，顶层键 `contractVersion` / `startupSnapshot` / `backendEvent` / `lessComputerVoice` / `androidJni` / `linuxFacade` / `enums`。启动时校验版本；UI 必须先消费 startup snapshot 再渲染状态。
- 构造：`LinuxBackendBuilder::from_shared_providers(config)`（`linux-egui/src/backend.rs`）创建与 Tauri 同源的 Core 后端；云 ASR/LLM/Omni 实现两端共用，不复制第二套。
- 事件：Core `events.rs` 语义事件 → `LinuxHost::drain_events`（`lib.rs`）→ UI reducer；订阅用 `LinuxHost::subscribe` 返回 `EventSubscription`。
- 会话/取消：所有长操作（听写、转写、下载、QA、Agent）走 Core session 取消语义；UI 只触发与显示，不自建取消规则。

## 2. Core 公开模块（`crates/openless-core/src/lib.rs`）

按域的模块地图见[架构文档](../architecture.md)第 3 节。Linux 直接相关：

- 业务门面：`api`（对外接口）、`domains`（领域出口）、`events`、`ports`（Host 注入接口，含 `NoopEditObservationAdapter` 等默认实现）。
- 听写：`dictation_engine`、`silence_auto_stop`、`streaming_insert`、`hotkey_interpreter`。
- 服务：`provider_*` 系列、`omni`、`credentials`、`endpoint_security`。
- 本地模型：`model_store`、`local_asr_service`、`local_asr_catalog`。
- 领域：`history`、`vocabulary`、`correction`、`style_pack_store`、`marketplace`、`qa_service`、`selection_*`、`less_computer`、`remote_input_service`。

## 3. 注入点（`ports.rs`）

Host 必须实现并注入：`AudioRecorder`、`TextPolisher`、`TextInserter`、`CredentialStore`、`SettingsRuntime`、`TaskSpawner`、`EditObservationAdapter`（未接时为 `NoopEditObservationAdapter`，见缺口 L02）、`LinuxHostActions`。注入清单与默认值以 `backend.rs` builder 为准；缺省实现表示"未接线"，不是"不支持"。

## 4. 纪律

- 保留启动、版本校验、事件与退出合同；改动先过 `cargo test -p openless-core --locked` 与合同测试。
- 合同测试不替代真实平台证据（见[07 验收](07-acceptance.md)）。
