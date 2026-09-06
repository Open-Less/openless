# OpenLess Linux egui 与共享后端 lib 拆分实施计划

> **2026-09-06 范围更新**：当前需求以[2.0交付范围](./2.0-requirements.md)、[Windows/macOS验收](./2.0-desktop-acceptance.md)及[Linux拆分交接目录](./linux-egui-handoff/README.md)为准。Windows/macOS须完整保留各自Tauri 1.x功能；Linux本批交付可接入Core，剩余Linux Host/UI和产品验收交egui团队，不再作为本批桌面2.0完成条件。下方旧阶段记录不覆盖这一新范围。

> 历史说明：本文保留 2026-08-31 至 2026-09-01 的分阶段设计与证据，正文中的“当前”、旧 HEAD、
> 测试计数和 UI stub 状态不代表 PR #1019 最新实现。当前 2.0.0 接口以
> `linux-egui-backend-contract.md`、代码中的 canonical fixtures 和 PR #1019 body 为准。

> 文档状态：实施与复审记录。2026-09-05 完整复核发现此前 F01–F24 的接口/fixture 通过仍遗漏生产 Adapter、取消、输入目标与 UI 状态序列错误；问题、修复及最新验证以 [`pr1019-2.0-final-review.md`](./pr1019-2.0-final-review.md) 为准。不能继续用下面的历史收口矩阵单独证明迁移完成。
> 更新日期：2026-09-05；真实设备、签名、安装、升级和回滚仍需单独证据，最终远端 run/head 以 PR #1019 body 和本轮复审记录为准。
> 范围：抽取无 Tauri 依赖的共享 Rust 应用核心；保留 macOS / Windows 的 Tauri 前端；为 Linux egui 前端准备公共接口、事件契约、测试适配器和构建契约
> egui 责任（2026-09-06调整）：由另一组负责剩余Linux Host原生实现/接线、egui/eframe界面、交互、视觉、设备与Linux发布验收；本轮交付稳定Core与细化缺口文档
> 最终审查基线：推送后重新读取 PR #1019 的真实 `baseRefOid/headRefOid`；不得继续使用历史 SHA 或把 `MERGEABLE` 等同发布就绪

## 0. F01–F24 收口矩阵（2026-09-04）

下表保留上一轮生产调用者与自动证据索引；本轮新发现与修复见上方链接。“设备待补”始终不等于真实平台或发布就绪。

| 项 | Core 生产调用链 / 已删除的 Host policy | 自动证据 | 真实平台证据 |
| --- | --- | --- | --- |
| F01 | Linux egui → Core Credential/Provider/LocalAsr activation；UI 不再持有 endpoint/model/auth 默认值 | provider、credential、Linux factory/UI contract | Ubuntu 首次配置待补 |
| F02 | Linux factory → Qa/Remote/Selection Adapter；AppImage installer 先于 listener | Linux lib/host contract、WSL 条件编译 | Ubuntu QA/Remote/preview/fcitx5 待补 |
| F03 | Tauri/Linux → `start_less_computer_voice` +统一 voice lease/cancel；Host 不再因 `take()` 丢取消路由 | Less Computer capture/cancel/approval tests | Windows/macOS 冷启动语音待补 |
| F04 | `CredentialDirectory` 持有 channel ID/type/order/enable/active；Tauri mutation policy 已删除 | provider channel facade、CredentialDirectory contract、source gate | Keychain/Secret Service A/B channel 待补 |
| F05 | ProviderDescriptor auth requirement → validate 与真实 builder；空 key 不发 Authorization | local fake HTTP no-auth ASR/LLM tests | 自建无鉴权服务待补 |
| F06 | ProviderDescriptor validation probe/static model policy → ProviderService | StepFun/DashScope/static-list/timeout/cancel/redirect tests | 各云厂商错误 key 待补 |
| F07 | 共享 `SilenceAutoStop` + typed RecordingPlan/Event/Control；Host 仅 recorder effect | 六个 silence tests、QA/Selection/Less wiring、recording fault test | 三平台 mute/fault/设备切换待补 |
| F08 | `TextInserter::begin` 固定 opaque target；Core HostContext/EditObservation generation | cursor-off零文档读取（前台应用仍冻结）、edit stale、target restore/source gate | Windows 切焦点、macOS AX/privacy 待补 |
| F09 | Pipeline correction → actual ASR/LLM labels/timing/history；Host 归因逻辑已删除 | corrected-polisher/history/provider attribution tests | Foundry GPU→CPU notice 待补 |
| F10 | `HotkeyInterpreter` 持有 press generation、grace/debounce/cooldown；Host 只发 edge | Core 29、Tauri 39、Linux hotkey contract | 三平台物理三连按待补 |
| F11 | Pipeline 从同一 archive/冻结 context 最多重试两次 | retry success/exhaust/cancel/terminal contract | 冷云 ASR 失败恢复待补 |
| F12 | H5 bounded pre-ACK queue → Core `RemoteFrameCodec`/session guard | Rust remote contract、浏览器 queue contract | 手机冷启动首词待补 |
| F13 | Core `MacosNewlineMode` 与 frozen front app；Host 只执行 LF/Return effect | serde、Terminal/非 Terminal、Unicode streaming tests | macOS Terminal/聊天框待补 |
| F14 | 唯一 `effective_pipeline_mode` 供 Dictation/QA/Selection/Auxiliary/status 使用 | flag/mode 与 credential zero-read tests | 运行中切换待补 |
| F15 | Core `PasteSent` 与 `OutcomeUnknown` 分离；不自动重试未知结果 | Core/Tauri mapping 与 contract fixture | Windows Paste/TSF 待补 |
| F16 | Core `ActiveTextInsertion` 独占 final reconciliation；两 Host 副本已删除 | streamed/tail/Unicode/diverge/fallback tests、source gate | Tauri/Linux 实际落字待补 |
| F17 | Tauri presenter 按 generation 延迟终态，fallback 卡持有窗口；主窗仍收 cue | capsule visibility/ownership/timing/message tests | Windows/macOS 可见时序待补 |
| F18 | Unicode replace-from TranscriptDelta + session/sequence reducer/replay | Rust canonical fixture、TS reducer、Linux lag replay tests | provider interim 修订待补 |
| F19 | startup 同时迁移默认与 custom ModelStore root | Qwen/Q5/Sherpa/conflict/idempotence tests | 1.x 用户目录升级待补 |
| F20 | Core LocalAsr activation transaction + target/generation lease | activation rollback/switch/release、Linux process-group tests | macOS/Windows runtime 切换待补 |
| F21 | ModelStore partial/ready/card 与 Linux 打包 Qwen runtime | model-store contract、package script/workflow checks | deb/rpm/AppImage 安装与性能待补 |
| F22 | Core AgentCommand/materialization/PATH/parser；Host 只做 I/O/spawn/kill | coding-agent parser/guard/PATH/process tests | 四 CLI 桌面启动待补 |
| F23 | 真实 StartupSnapshot/BackendEvent DTO；所有 React webview 与 Android fail-closed | Rust canonical fixture、TS gate、Android snapshot tests | Android overlay/IME 待补 |
| F24 | Credential/QA/Selection/stream/provider/history 旧 Host policy 同阶段删除 | `shared-backend-wire-contract.test.mjs` residual allowlist | 不适用；随源码复审 |

## 1. 执行摘要

当前仓库已经有 Cargo `lib` crate，但这个 `lib` 仍然是 Tauri 应用本身：应用入口、Tauri builder、插件、窗口、托盘、IPC 命令注册和部分核心协调逻辑都在同一层。目标不是把现有文件整体搬家，而是建立一个真正不依赖 Tauri 或 egui 的 `openless-core`，再由两个宿主适配器使用它。

架构结论：原方案方向正确，但“整个后端包装进一个 lib”应理解为“把跨平台业务和状态机
收敛到一个深的 core Module”，而不是把所有 OS 能力塞进同一个 crate。麦克风、文本插入、
热键、凭据存储、窗口、托盘、单实例和更新器仍通过 core 拥有的最小 Interface 由平台
Adapter 实现；否则只是把 Tauri 耦合从应用入口搬进 library，Linux 仍无法真正独立。

目标结构：

```text
React UI ── Tauri 适配器 ──┐
                           ├── openless-core
egui UI  ── Linux 适配器 ──┘
```

核心规则：

1. `openless-core` 只承载应用业务、会话状态机、provider 调度、持久化、类型化结果和语义事件。
2. Tauri 只承载 IPC 转换、Tauri 插件、窗口/托盘生命周期和 WebView 事件桥接。
3. Linux适配器只承载Linux平台能力和非UI宿主接线；依2026-09-06范围，其剩余工作与egui/eframe主循环、窗口及交互一并交egui团队实现。
4. 核心接口不能出现 `AppHandle`、`tauri::State`、WebView 窗口 label、`emit_to` 或 egui 类型。
5. egui 团队只接收稳定的 Rust 接口和事件契约，不需要了解核心内部模块。
6. Android 暂时继续作为 Tauri mobile 适配器；本计划不把 Android UI 改成 egui，也不改变现有 Android 语义。

### 1.0.1 责任矩阵与移交门

| 责任方 | 本计划内的交付 | 明确不负责 |
| --- | --- | --- |
| 共享后端/架构组 | `openless-core`真实领域实现、facade、状态机、平台Interface、事件/错误/能力合同、fixture/headless、兼容测试与Linux拆分交接资料；修复接入所需Core缺口 | Linux剩余Host/UI及Linux完整产品验收 |
| Tauri组 | Windows/macOS完整保留各自1.x功能，React IPC/event接线、原生Adapter、自动与设备/安装验收；保留现有Android合同不回退 | 在command中重新实现Core业务规则；本次不新增Android首批完整支持承诺 |
| Linux Host工作（移交egui团队） | 复用已有Secret Service、fcitx5、cpal、单实例/runtime；补原生效果、全局热键、上下文/归档、系统集成与设备/发布验收 | 重写Core领域规则或将OS细节塞进Core |
| egui组 | 承接上一行Host工作，并基于2.0.0合同补齐页面、交互与Linux产品验收；缺口逐项见交接目录 | 读取Core私有模块、include Tauri源码、复制业务规则，或在UI绕过LinuxHost事务/读取秘密 |

**当前移交门**：按[2.0需求第4节](./2.0-requirements.md#4-core-对-linux-的交付门)提供真实Core实现、可运行合同/示例与[拆分交接资料](./linux-egui-handoff/README.md)。保留已有Linux实现；未接入的原生效果明确表达不可用并移交，不等待Linux全部设备/UI验收才交付Windows/macOS。后文M7为历史阶段记录。

完成的定义：

- macOS / Windows Tauri 功能仍通过原有 React IPC 契约工作。
- Linux egui 程序可以直接依赖 `openless-core`，不编译 Tauri、WebKitGTK 或 Tauri plugin。
- 两个宿主都能使用同一套听写、润色、设置、历史、词典、风格包和 provider 业务规则。
- 核心可以在无窗口、无 WebView、无真实麦克风的测试环境中通过 fake adapter 验证。
- egui 团队获得版本化的公共接口、事件语义、能力矩阵、错误码、示例 host 和 headless 测试夹具。

### 1.1 当前实施状态（2026-08-31）

此表只描述当前工作树中的可验证状态，不替代后面的最终验收清单：

状态统一使用以下口径：`已完成` 表示该里程碑的退出条件和当前相关门禁均已通过；
`已完成（Interface）` 只表示 egui 团队可以依赖的 Interface 已稳定，不表示真实 Linux 原生能力或
正式产物已经验收；`进行中` 表示仍有源码、兼容、原生或发布证据缺口。任何会影响既有证据的
后续改动都会使对应门禁重新变为待验证，历史测试数字只保留作参考，不能继续标为“最新通过”。

| 阶段 | 状态 | 当前证据 | 仍需完成 |
| --- | --- | --- | --- |
| M0 | 已完成 | 196 command / 30 legacy event / 29 core event kind 的机器基线、drift check 和平台/调用/版本决策已冻结 | 只有破坏性 Interface 变更才重新打开决策 |
| M1 | 已完成 | 根 workspace 仅含 core/Linux；Tauri 与 backend compatibility tests 使用独立 manifest/lockfile；依赖门禁通过 | 对应原生 target 仍由 CI 证明 |
| M2 | 已完成（Interface） | 共享 `types.rs` 已由 core 类型重导出；听写、设置、历史、词典、风格包、凭据及复杂领域 DTO/错误/serde fixture 已建立；快捷键语法、冲突规则、完整设置 DTO 和 Linux validated settings 公共 contract 已进入稳定 Interface；style-pack prompt 诊断和 ASR 热词排序也已收敛到 Core facade | 破坏性 Interface 变更才重新打开版本迁移 |
| M3 | 已完成（Interface） | lifecycle、`DictationEngine`、`AudioRecorder`、`TranscriptionEngine`、`TextPolisher`、progress sink、host/inserter/credential/resource Interfaces、`BackendServices` 与 fake/unsupported Adapter 已有 | 新 seam 仍须满足“两个真实 Adapter，或一个真实 Adapter 加一个测试替身”的建立条件 |
| M4 | 进行中 | core 已有完整 Pipeline、录音 level、sequence/session/lagged/late-result/cancel race tests；成功/失败 history、实测 ASR/润色耗时、失败录音保留、成功录音隐私清理和最终纠正规则已由 core 统一；backend 实例拥有 2048 条有界事件 replay，Less Computer 不再使用进程级静态 backlog；Android stop-time translation、remote external PCM、桌面普通听写的 Core Pressed/Released/Combined 和 Esc 取消已使用冻结 session context 与共享 Core 状态机；QA 的 phase/message/cancel/conversation 真相也已归 `QaService`，窗口可见性由 Coordinator/QA Adapter 共享的 `TauriQaHostContext` 持有，不再存在第二份 `QaHostState`；30 个 legacy event 已完成归类，原 12 个待迁移事件已获得 typed core event；`Coordinator::Inner` 与 `capsule_focus` 已恢复 module 私有，`bind_app(AppHandle)` 已删除，Coordinator/capsule 子模块中的 `AppHandle`、`WebviewWindow` 和直接 Tauri emit 已清零；capsule 的原生窗口操作、布局/穿透/style/fallback cache 与 deferred payload 均归 `TauriCoordinatorHost`，`TauriCapsuleWindow::apply_capsule_payload` 只接收窄值 | Core `LessComputerVoiceSession` 现统一 capture lease、ASR 取消、PCM 校验、TranscriptDelta final 和 Agent submit；Linux/Tauri edge 仅做宿主生命周期适配，pending stop 与静音策略继续由各自 Host 触发 Core；旧 compatibility `Coordinator` 仍持有显式 Tauri Host，并承担部分热键仲裁、native runtime 生命周期和兼容编排；继续完成宿主边界审计并取得 Android/macOS/Windows/Ubuntu 原生验证 |
| M5 | 进行中 | preferences/history/activity/vocabulary/correction/style-pack/ZIP/output-cleaning/credentials、完整 prompt compose、实时云 ASR/LLM/Omni 协议、provider 默认值/凭据路由/取消与 QA answer 已迁入 core；设置事务现由 Core 统一 legacy 同步、strict/reconcile、style 保留、typed effect plan、单写入 gate、乐观 revision、一次持久化/事件及 receipt 补偿；Selection Voice 的 transcript correction、instruction polish、自动意图模型/fallback、输出模式、EditPlan、translation 与 QA preview 迭代现由 Core 高层 use-case 统一；Tauri/Linux Adapter 只消费显式 action/target；Coding Agent、Local ASR、Marketplace、Selection、Selection Voice、QA、Remote Input 及 Provider 管理面（`ProviderService::validate/list_models`）均已有 Core Implementation | native/local ASR、平台录音、socket、窗口、授权和系统 effect 继续留在 Adapter；Selection Voice/QA/Remote Input、Provider 真实网络和设置原生 effect 仍需完整平台证明 |
| M6 | 进行中 | Tauri 已管理 `Arc<OpenLessBackend>`；生产云 ASR/LLM/Omni/Auxiliary/QA/Provider 管理面的运行时均调用 core 共享 Implementation，Tauri 只注入 `SystemCredentialStore`、平台录音、native/local ASR、窗口/插入与系统 runtime；Selection Voice Adapter 只提交原始 transcript、执行 Core `SelectionVoiceEditAction`、保存 opaque insertion target 并回报 apply outcome，源码门禁禁止业务规则回流；React command、CLI、Android JNI、remote WebSocket PCM、桌面普通听写热键及复杂领域的业务调用均调用 core Interface；Core `LessComputerVoiceSession` 已可供非 Tauri Host 使用并固定 provider/model、审批、continuation、stream 和终态，旧 Coordinator 语音 recorder/ASR 兼容编排仍待替换；Host 只注入 recorder/native runtime 与热键边沿；legacy provider/Selection Voice 业务副本及仅供历史测试使用的 coordinator runner/approval helper 已删除 | Provider command 已收窄为参数/错误转换，Linux factory 已接入同一 Core service、Generic/Qwen ASR CLI 和 Coding Agent runtime；继续收窄旧 Coordinator 兼容 host state，并补齐 Android/macOS/Ubuntu 原生证据 |
| M7 | 已完成（Interface） | `BackendServices` 全领域 Interface、完整 headless/unsupported 示例、Linux host contract、能力 fixture 和 unsupported 语义已交付；`LinuxHost::save_settings`/`update_settings_strict` 强制 snapshot revision；4 项公共 host contract 已覆盖设置事务以及 Selection/Selection Voice 的 preview、confirm、cancel、stale、outcome-unknown 与 Linux preview/revert `Unsupported`；Provider 管理面已有 Core/Tauri/Linux 接线和源码契约；当前公共面门禁和 headless 示例运行通过 | egui/eframe UI、交互、视觉与 UI 验收由另一组负责，不属于本交付 |
| M8 | 进行中 | Linux Secret Service、资源布局、fcitx5 插入、cpal 录音、DBus 热键 listener、HostActions、能力矩阵、单实例与统一 `LinuxNativeRuntime` 已实现；`LinuxBackendBuilder::from_shared_providers(config)` 无需 UI 注入 provider factory，即可组装共享云 ASR/LLM/Omni/Auxiliary、ProviderService、Marketplace、传统 Pipeline、凭据和 settings runtime；`LinuxHost` 暴露同一 ProviderApi；`LinuxHost::download_marketplace_archive` 提供不覆盖已有文件的 filesystem sink；`LinuxSettingsRuntime` 按 receipt 恢复显式 effect；WSL Ubuntu 已通过真实 Secret Service adapter 的 set/read/remove + secret 边界 contract、fcitx5 插件加载/DBus method/listener/press-release-combined-translation signal contract、cpal 无输入设备的稳定错误分类，以及 desktop/AppStream metadata 校验 | 仍需真实焦点输入上下文中的按键/translation 顺序、存在音频设备时 cpal start/stop、settings effect/单实例退出的桌面流程，以及正式 Ubuntu runner 的安装/签名证据；合成 DBus signal 和 WSL contract 不能替代这些证明 |
| M9 | 进行中（CI runner 门禁已通过；原生安装/设备证据待完成） | fork CI run 33408317390（head `06e85f7b`）四个平台 job 全部成功：Linux `openless-core` 596 unit + 79 contract、Linux crate 30 + 4 host contract（3 个真实 Linux native contract 明确 ignored）；macOS Tauri 737 tests（730 passed、7 ignored）、Windows Tauri/Core checks、Android `aarch64`/`x86_64` mobile compile/Gradle/JVM/instrumentation/Keystore contract；frontend/contract 58、196/30/29 基线、依赖/秘密/隔离/runtime/public-surface/source/headless 等门禁均通过 | 仍缺 Android 签名安装/设备运行、macOS/Windows 安装升级 smoke、Ubuntu 真实桌面输入/音频/设置流程及正式 runner 的签名安装证据；Linux UI stub 仍不属于产品验收 |
| M10 | 进行中（验证产物已可生成；正式发布待外部门） | Tauri/Linux release workflow 已拆分；Linux deb/rpm/AppImage/fcitx5/updater 契约和 README/RELEASING 已加入；CI run 33408317390 的 Linux artifact job 成功上传并校验 `openless-linux-egui-x86_64`（artifact ID 9764249814），手动 Tauri/Android workflow 也已分别生成桌面和四 ABI debug artifact | UI stub 未替换，故 Linux workflow 不监听 tag；正式签名密钥、真实 Ubuntu runner 安装/运行 proof、正式 macOS/Windows/Android 签名安装仍缺 |

### 1.1.1 当前 Coordinator 收口边界

本节是对上表中 M4/M6 状态的代码级澄清，防止把“Core 已提供接口”误读成“所有生产入口都已迁移”。

- Less Computer 的文字入口，以及宿主完成录音/ASR 后的
  `run_voice_agent_transcript -> submit_less_computer_with_session`，已经使用 Core 的
  provider、prompt、护栏、审批、continuation、stream 和终态规则。
- Less Computer 的热键按下会先调用
  `OpenLessBackend::begin_less_computer_capture` 预留 Core capture lease，再由
  [`coordinator/hotkey_loops.rs`](../openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L779-L849)
  以同一个 session id 启动兼容 Coordinator 的 recorder/ASR；松开、Starting pending stop、
  静音自动停止最终仍复用 `end_session`，但转录提交、Agent 运行、审批、取消和终态全部由
  Core 负责。启动失败、空转写和取消会释放未提升的 capture lease。
- Coordinator 的 `state`、`voice_agent`、`pending_stop` 仍是 Tauri 兼容层的录音/热键状态，
  不是 Linux egui 可见的业务真相；Linux host 应直接使用 Core facade 的 active session、
  cancellation 和 typed events，不读取这些字段。普通听写的 Pressed/Released/Combined/Esc
  继续使用 Core dictation 状态机。
- QA 编辑预览需要把平台的 opaque selection target 绑定到 Core preview；该动作现在由构造阶段
  注入 `TauriQaHostContext` 的弱引用回调完成。QA Adapter 不再通过 `AppHandle.try_state` 反查
  `Coordinator`，并由 source contract 和 focused test 守护这一边界。
- 本批迁移的完成判据是：Less Computer 的跨宿主身份使用同一个 Core session id，任何 Host
  只负责捕获资源和生命周期边沿；Provider、prompt、approval、continuation、stream、终态
  和 cancellation 语义只能由 Core 产生，并由 headless/compatibility contract 覆盖。

### 1.2 本轮原生验证记录（2026-08-29）

以下 Linux 证据来自 WSL2 Ubuntu（不是 Windows 交叉编译），Android Rust cross-target 证据来自
当前 Windows 主机；临时 minisign key 只位于 WSL `/tmp`，未写入仓库：

- `dbus-run-session` + `gnome-keyring-daemon --unlock --components=secrets` 下，`secret_service_contract` 以 `--ignored` 显式通过 1 项：adapter 实际写入、读取、删除 Secret Service 项，metadata 文件不包含 secret。
- 系统安装并加载仓库构建的 `libopenless.so` 后，`fcitx5` 真实 DBus 对象注册成功；`fcitx5_contract` 以 `--ignored` 通过 1 项，覆盖 `Fcitx5HotkeyListener` 启停、DBus method、press/release/combined/translation signal 顺序映射和 no-focused-input 的安全返回。`CommitText(s: text) -> b` 的 `false` 被 Rust Adapter 转为明确的平台/插入失败，不能当作成功；测试后 fcitx5 进程仍存活。真实桌面输入上下文的物理按键顺序仍需 runner 证明。
- `cpal_contract` 以 `--ignored` 通过 1 项；WSL 当前无 ALSA 输入设备，adapter 返回 `Platform`/`PermissionDenied`/`Unsupported` 中的明确错误而不是 panic 或假成功。存在真实设备时的 stream start/stop 仍需 runner 证明。
- Android Tauri mobile 初始化、脚手架复制和 manifest 合并脚本均已在当前工作树执行成功；随后使用
  `cargo ndk -t arm64-v8a check --manifest-path "src-tauri/Cargo.toml"` 与
  `cargo ndk -t x86_64 check --manifest-path "src-tauri/Cargo.toml"` 通过，证明两个 Android
  Rust target 的源码/依赖可编译。该证据不包含 JNI 在设备上的运行、Gradle/JVM 编译、APK/AAB
  组装、instrumentation 或签名安装；本机 Gradle cache 缺少
  `com.android.tools.build:gradle:8.11.0`，在线解析未稳定完成，因此这些项目必须由具备完整
  Android/Gradle cache 的 CI runner 证明。临时生成目录中的 AGP 版本尝试已恢复为仓库声明版本，
  未修改 CI 版本契约。
- `desktop-file-validate` 与 `appstreamcli validate --no-net` 通过；AppStream metadata 已补齐 description/homepage。
- 重新生成的 Linux 产物位于 `openless-all/app/target/linux-egui-packages/`：deb、rpm、AppImage；release binary/plugin 的 `ldd` 无 `not found`，且无 Tauri/Wry/WebKit 依赖；deb/rpm/AppImage 内容均含 binary、desktop/AppStream metadata 和 fcitx5 plugin。使用临时 minisign key 对 AppImage 的签名/验签已通过；独立 updater manifest 由 release workflow 生成，正式发布必须注入正式 secret，当前不把临时签名当作可发布凭据。

### 1.2.1 历史 Windows 本地重验（2026-08-30）

本轮只记录当前工作树可在 Windows 主机复现的证据；它不能替代 Android、macOS 或真实
Ubuntu 桌面 runner 的原生证明：

- `npm.cmd test`（包含 pretest build）退出码为 0，发现并执行 58 项前端/契约测试；
- `cargo test --locked --manifest-path "src-tauri/Cargo.toml" --lib` 运行 752 个单元测试，
  结果为 745 passed、0 failed、7 ignored；Provider 旧 command 测试旁路已删除，解析与模型
  响应测试归入 Core `ProviderService`。
- `cargo test --locked -p openless-core` 运行 594 个 unit tests，领域 integration contract
  另有 79 项，全部通过；
- `cargo test --locked -p openless-linux-egui --all-targets` 运行 29 个 Linux crate tests 和
  4 个 host contract tests；Secret Service/fcitx5/cpal 原生 contract 在 Windows 以 0 tests
  保持 ignored，不被误报为 Linux 原生成功；
- Core/Linux clippy（`-D warnings`）、workspace fmt、command/event baseline（196/30/29）、
  core/Linux 依赖、secret surface、test isolation、runtime seam、Linux public surface、
  source contract、headless example 和 `git diff --check` 均通过；
- 修正 `release-linux-egui.yml` 的版本解析路径：该步骤的
  `working-directory: openless-all/app` 现在读取 `src-tauri/Cargo.toml`，不会再拼出重复的
  `openless-all/app/openless-all/app` 路径；本地已用同一工作目录解析出
  `1.3.18-Beta.7`。

以上结果证明共享 core、Linux Interface 和 Tauri compatibility 在当前工作树可构建并通过
本地契约；不证明真实音频设备、焦点输入、fcitx5 物理按键顺序、安装/签名、Android APK/JNI
或 macOS/Windows 安装包行为。

### 1.2.2 跨平台 CI runner 验收（2026-08-31）

提交 `06e85f7b8b9e93db7df276952a18825e245e7c37` 在 fork 的 [CI run 33408317390](https://github.com/H-Chris233/openless/actions/runs/33408317390) 上四个平台及 Linux artifact job 全部成功：

- Linux core and egui host：Core 596 unit + 79 contract、Linux crate 30 tests + 4 host contract、严格 clippy 和依赖/秘密/隔离/runtime/public-surface 门禁通过。
- Android cargo check：`aarch64`/`x86_64` Tauri Rust check、Gradle scaffolding、JVM unit/instrumentation tests 和 Android Keystore instrumentation 通过。
- Windows checks：前端/契约 58 项、Tauri check、Windows backend test compile、Rust-only backend tests、Core contract、Rust 1.88 MSRV 和五处版本同步通过。
- macOS checks：前端/契约 58 项、Qwen3/Tauri check、737 个 Tauri Rust unit tests（730 passed、7 ignored）、Rust 1.88 MSRV、backend test compile 和版本同步通过。
- Linux egui validation artifact：无 Tauri 的 deb/rpm/AppImage、fcitx5 plugin、ELF/包内容/desktop/AppStream 和 updater manifest SHA-256 校验通过。

该 run 证明当前提交在声明的原生 runner 上可编译并通过已配置的契约；它不等同于正式 release workflow 的安装包、签名、设备输入/音频或 egui UI 视觉验收。Linux egui UI 仍由 egui 组实现，正式发布仍按 M10 的 release gate 执行。

### 1.2.3 远端验证 artifact（2026-08-31）

- Linux artifact job [run 33408317390](https://github.com/H-Chris233/openless/actions/runs/33408317390) 成功上传 artifact `openless-linux-egui-x86_64`（ID `9764249814`），包含 1 个 deb、1 个 rpm、1 个 AppImage 和 `latest-linux-egui-x86_64.json`；下载后的 AppImage SHA-256 `f9e061c7b27ba26eff886a68b1acaaf561a67389447f485b8593a02e341a9307` 与 manifest 一致，manifest URL 指向 `H-Chris233/openless`。
- Tauri 手动构建 [run 33405500864](https://github.com/H-Chris233/openless/actions/runs/33405500864)（commit `80be78c2`）三个 job 全部成功，上传 macOS arm64/x86_64 DMG（artifact IDs `9757421887`、`9757475524`）和 Windows x64 NSIS 安装包（ID `9757290871`）；Windows runner 的 NSIS 安装/卸载与 IME smoke 通过，非数字 Beta 版本明确跳过 MSI。
- Android 手动构建 [run 33405500972](https://github.com/H-Chris233/openless/actions/runs/33405500972)（commit `80be78c2`）成功上传四个 ABI debug APK（artifact IDs `9757082911`、`9757086428`、`9757090328`、`9757094381`）；`Collect split APKs` 已校验每个 APK 只包含一个预期 ABI，artifact 均未过期。

以上是 CI 验证 artifact，不是正式签名发布：当前 Linux UI 仍是 stub，Tauri/Android 手动构建未注入正式签名密钥；真实设备安装、升级/回滚和 Ubuntu 桌面输入/音频仍由 12.4 未勾选门禁负责。

### 1.3 外部依据与本项目决策映射

本节只记录用于验证分层方向的公开一手资料；具体接口、兼容字段和完成状态仍以仓库代码、contract tests
和对应平台 runner 为准（资料核对日期：2026-08-29）。

| 依据 | 可采用的事实 | 对本项目的决策 |
| --- | --- | --- |
| [Tauri Architecture](https://v2.tauri.app/concept/architecture/) | Tauri 的 Rust 应用层、WebView 前端以及 command/event 通道属于宿主运行时边界 | `src-tauri` 只做 IPC、WebView、窗口、托盘、插件和移动端宿主；业务状态不能反向依赖 Tauri 类型 |
| [Cargo Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) | workspace 成员共享解析/构建上下文；`exclude` 和独立 manifest 可隔离不应参与某一构建的 package | Linux workspace 只解析 `openless-core` 与 Linux adapter；Tauri 保持独立 manifest/lockfile，避免 Linux 构建解析 Tauri/native path dependency |
| [eframe API](https://docs.rs/eframe/latest/eframe/) / [egui API](https://docs.rs/egui/latest/egui/) | eframe/egui 提供 native/web GUI application loop、绘制和 UI 状态承载 | `egui`/`eframe` 只进入 Linux UI crate；core 只交付同步快照、非阻塞事件订阅、异步 use-case 和 host action，不规定布局或视觉 |

由上述事实得到的项目结论是：用户提出的“后端包装进 lib、Tauri 薄包装、Linux 用 egui”方向正确，
但 lib 必须是“业务核心 + 由 core 所有的最小平台 Interface”，不能把窗口、热键、录音、凭据、
文本插入等 OS 实现继续塞进同一个跨平台 crate。否则 Linux 虽然不直接编译 Tauri，仍会被错误的
宿主耦合或不可替换的系统实现卡住。

## 2. 问题定义与现状证据

| 事实 | 位置 | 影响 |
| --- | --- | --- |
| Cargo 已声明 `openless_lib`，但主依赖包含 Tauri | [`src-tauri/Cargo.toml:9-23`](../openless-all/app/src-tauri/Cargo.toml#L9-L23) | 现有 `lib` 不是框架无关核心 |
| `run()` 直接分派到 `run_desktop()`，并在其中创建 Tauri builder | [`src-tauri/src/lib.rs:144-153`](../openless-all/app/src-tauri/src/lib.rs#L144-L153)、[`lib.rs:494-519`](../openless-all/app/src-tauri/src/lib.rs#L494-L519) | 启动和业务模块无法被 egui 直接复用 |
| 原 `Coordinator::Inner` 直接保存 `AppHandle`；当前已替换为显式 `TauriCoordinatorHost`，`Inner`/`capsule_focus` 已恢复私有，`bind_app(AppHandle)` 已删除，Coordinator/capsule 子模块中的 `AppHandle`、`WebviewWindow`、直接 emit 和直接 `tauri::async_runtime` 调用均已清零；原生 capsule window code/cache 已移入 Host | [`coordinator.rs`](../openless-all/app/src-tauri/src/coordinator.rs)、[`tauri_coordinator_host.rs`](../openless-all/app/src-tauri/src/tauri_coordinator_host.rs) | 窗口、事件和运行时 seam 已显式隔离；compatibility Coordinator 仍承担部分 Tauri-only 热键仲裁、native runtime 生命周期和兼容编排，尚未达到删除兼容层的终态 |
| 命令层以 `State`、`AppHandle`、`Window` 作为参数 | [`commands/mod.rs:11-24`](../openless-all/app/src-tauri/src/commands/mod.rs#L11-L24)、[`commands/mod.rs:130-148`](../openless-all/app/src-tauri/src/commands/mod.rs#L130-L148) | Tauri command 不是可移植的公共接口 |
| 兼容面包含 30 个旧 Tauri event，现已全部分类；原 12 个待迁移事件已集中映射 | [`linux-egui-command-event-baseline.json`](./linux-egui-command-event-baseline.json)、[`tauri_events.rs`](../openless-all/app/src-tauri/src/tauri_events.rs) | 后续新增业务事件必须先定义 core 语义事件，再由各宿主映射；纯窗口事件继续只归 Tauri host |
| Linux 入口包含 WebKitGTK compositing workaround | [`src-tauri/src/main.rs:4-20`](../openless-all/app/src-tauri/src/main.rs#L4-L20)、[`lib.rs:694-729`](../openless-all/app/src-tauri/src/lib.rs#L694-L729) | 分离 egui 的主要动机是降低 Linux WebView 风险，但 Wayland 仍需单独验证 |
| 原 Linux fcitx5 资源安装从 Tauri 取路径；当前 `openless-linux-egui` 已用 `LinuxResourceLayout`/`FcitxPluginInstallPlan` 独立实现 | [`linux-egui/src/resources.rs`](../openless-all/app/linux-egui/src/resources.rs)、[`linux-egui/src/fcitx5.rs`](../openless-all/app/linux-egui/src/fcitx5.rs) | Linux package 不再依赖 Tauri；真实安装顺序仍需 Ubuntu proof |
| 旧 Rust-only backend test 曾通过 path include 和 Tauri stub 绕开完整应用；当前该旁路已删除 | [`backend-tests/Cargo.toml`](../openless-all/app/src-tauri/backend-tests/Cargo.toml)、[`core_contract.rs`](../openless-all/app/src-tauri/backend-tests/tests/core_contract.rs) | compatibility package 现在只验证公开 core contract；原 Tauri 单测必须在 Tauri crate 自身运行，不能再复制源码 |
| React IPC 已按领域拆成多个模块 | [`src/lib/ipc/index.ts:1-20`](../openless-all/app/src/lib/ipc/index.ts#L1-L20) | 可保留现有 command 名称，降低 Tauri 迁移风险 |

历史发布工作流曾把 Linux 放在 Tauri 矩阵中并安装 WebKitGTK。当前工作树已经把 Linux 从
[`release-tauri.yml`](../.github/workflows/release-tauri.yml) 移出，并建立独立的
[`release-linux-egui.yml`](../.github/workflows/release-linux-egui.yml)。在真实 egui 入口替换
stub 前，Linux workflow 只允许手动或复用调用，不能由 release tag 自动发布。

## 3. 目标与非目标

### 3.1 目标

- 建立不依赖 Tauri / egui 的 `openless-core` Rust library。
- 把 Coordinator、ASR/LLM pipeline、持久化和业务类型放到 core 的清晰模块中。
- 通过类型化接口提供同步查询、异步命令、取消、快照和事件订阅。
- 用宿主适配器承载窗口、托盘、权限、更新、开机自启、单实例和系统集成。
- 保留现有 React IPC command 名称及其 JSON 字段兼容性，作为 Tauri 适配器的兼容层。
- 为 Linux egui 团队提供可独立开发的接口包、示例、mock、事件映射和 headless 验证。
- 把现有 Rust backend tests 迁移为 core 的单元测试和 adapter integration tests。
- 在 CI 中证明 Linux egui package 的依赖树没有 Tauri/WebKitGTK。

### 3.2 非目标

- 本计划不实现 egui 页面、视觉设计、控件、布局、动画或 UI 自动化。
- 不要求 React 与 egui 像素级一致；只要求业务语义和能力契约一致。
- 不把所有代码强行合并为一个几千行的 `lib.rs`；一个 library 可以内部由多个深模块组成。
- 不在第一阶段将后端拆成大量独立远程进程或引入 JSON/RPC；同进程 Rust 调用应保持类型化。
- 不改变 Android 当前 Tauri mobile 适配器的产品行为。
- 不顺手修改 provider 协议、ASR 模型、提示词、发布版本号或无关 UI 行为。

## 4. 目标包结构

### 4.1 推荐目录

在 `openless-all/app` 建立只包含 core/Linux 的 Cargo workspace；现有 `src-tauri` 和
`src-tauri/backend-tests` 各自保留独立 manifest/lockfile。这样执行 Linux package 命令时，
Cargo 不会为了加载 workspace 元数据而解析 Tauri 的 macOS-only path dependency：

```text
openless-all/app/
  Cargo.toml                         # core/Linux workspace root 与独立 Cargo.lock
  crates/
    openless-core/
      Cargo.toml                     # 不出现 tauri、egui、eframe
      src/
        lib.rs
        api.rs                       # 对外 facade 和 use-case 接口
        events.rs                    # 语义事件和事件订阅
        errors.rs                    # 稳定错误码
        config.rs                     # BackendConfig / 路径 / 能力
        types.rs                      # 跨宿主共享 DTO 和领域类型
        coordinator/
        asr/
        polish/
        persistence/
        providers/
        ...
  src-tauri/                         # 根 workspace 显式 exclude
    Cargo.toml                       # 独立 Tauri Adapter + macOS/Windows/Android host
    Cargo.lock
    src/
      main.rs                        # Tauri desktop/mobile entry
      lib.rs                         # Tauri setup、commands、window/tray lifecycle
      commands/                      # 薄 command adapter
      tauri_events.rs                # core event -> WebView event bridge
      host/                           # Tauri-specific host actions
  linux-egui/
    Cargo.toml                       # 当前为非 UI Linux Adapter；egui 团队在此接入 eframe
    src/
      main.rs                        # 由 egui 团队实现
      host.rs                        # Linux host adapter；本计划提供接口
  src-tauri/backend-tests/           # 独立 compatibility-test package
      Cargo.toml
      Cargo.lock
```

当前已建立隔离后的 workspace 骨架：[`openless-all/app/Cargo.toml`](../openless-all/app/Cargo.toml)、
[`openless-core`](../openless-all/app/crates/openless-core/)、
[`openless-linux-egui`](../openless-all/app/linux-egui/)。Tauri 通过 path dependency 使用
core，但不会成为 Linux workspace 的解析依赖。该结构不代表 Coordinator 和复杂 provider
实现已经完成迁移；实际状态以 1.1 表为准。

### 4.2 依赖方向

```text
openless-core
  ├── serde / tokio / reqwest / persistence dependencies
  ├── platform Interfaces (traits owned by core)
  └── no Tauri, no egui, no WebView type

openless-tauri（现有 Cargo package 名称可继续为 openless） ──> openless-core
  ├── Tauri commands / plugins / windows / tray
  └── React IPC event names

openless-linux-egui ──> openless-core
  ├── 当前：Linux credentials / fcitx5 / resources / capabilities / host actions
  └── 后续由 UI 团队加入 eframe / egui 与窗口/托盘交互
```

禁止以下反向依赖：

- core `use tauri::*`
- core `use egui::*` 或 `use eframe::*`
- core 读取窗口 label（`main`、`capsule`、`qa`、`less-computer`）
- core 直接 `emit_to`、创建 WebView、调用 Tauri plugin
- Linux egui crate 通过 path include 复用 `src-tauri/src/*.rs`
- Tauri adapter 把业务判断重新实现一遍，导致第二份真相

## 5. 共享核心接口设计

以下 Interface 已在 M0/M1 冻结；后续只能按 contract version 规则演进，不能由某个宿主单方面改名或改变语义。

### 5.1 Backend facade

核心对宿主提供构造、生命周期、快照、事件和领域 use-case 的 facade；复杂领域通过
`BackendServices` 暴露稳定 Interface：

```rust
pub struct OpenLessBackend { /* private state and adapters */ }

impl OpenLessBackend {
    pub fn new(config: BackendConfig, deps: BackendDependencies)
        -> Result<Self, BackendError>;

    pub async fn start(&self) -> Result<StartupSnapshot, BackendError>;
    pub async fn shutdown(&self) -> Result<(), BackendError>;

    pub fn snapshot(&self) -> BackendSnapshot;
    pub fn subscribe(&self) -> EventSubscription;
    pub fn services(&self) -> &BackendServices;

    pub async fn start_dictation(&self) -> Result<SessionId, BackendError>;
    pub async fn stop_dictation(&self) -> Result<DictationResult, BackendError>;
    pub async fn cancel_dictation(&self, session: Option<SessionId>)
        -> Result<(), BackendError>;

    pub async fn submit_less_computer(&self, transcript: String)
        -> Result<LessComputerRunResult, BackendError>;
    pub fn begin_less_computer_capture(&self, session: SessionId)
        -> Result<(), BackendError>;
    pub fn less_computer_active_session(&self) -> Option<SessionId>;
    pub fn less_computer_capture_cancelled(&self, session: SessionId) -> bool;
    pub fn abort_less_computer_capture(&self, session: SessionId)
        -> Result<(), BackendError>;
    pub async fn submit_less_computer_with_session(
        &self,
        session: SessionId,
        transcript: String,
    ) -> Result<LessComputerRunResult, BackendError>;
    pub async fn cancel_less_computer(&self, session: Option<SessionId>)
        -> Result<(), BackendError>;
}
```

以上 facade 已在 `openless-core` 中落地。`BackendServices` 当前包含 `ProviderApi`、
`LocalAsrApi`、`SelectionApi`、`QaApi`、`RemoteInputApi`、`MarketplaceApi`、
`CodingAgentApi`、`PlatformApi` 和 `AuxiliaryApi`。宿主尚未注入的领域统一返回稳定的 `Unsupported`，
不得伪造成功；这让 egui 团队可以先针对 Interface 编写 view-model tests，而不会误判运行能力。

约束：

- `OpenLessBackend` 必须 `Send + Sync`，可以被 Tauri `State<Arc<_>>` 或 Linux host 持有。
- `new()` 不应隐式创建窗口、弹权限或启动全局热键；启动副作用由 `start()` 和 host lifecycle 明确触发。
- `shutdown()` 必须幂等，重复调用不能 panic，也不能遗留录音、热键、下载或 provider task。
- `snapshot()` 返回可安全克隆的 owned 数据；不暴露内部锁、引用或生命周期参数。
- 所有长任务必须异步执行；宿主 UI 线程不能等待网络、模型加载、录音或磁盘 I/O。

### 5.2 领域接口分组

不要把现有 196 个 Tauri command 原封不动暴露为 196 个核心方法。按当前 IPC 领域模块形成稳定的 use-case Interface：

| 领域接口 | 负责内容 | 典型操作 |
| --- | --- | --- |
| `DictationApi` | 听写会话和插入结果 | start/stop/cancel、状态快照、会话结果 |
| `SettingsApi` | 用户偏好和默认模式 | get、patch、默认风格提示词 |
| `CredentialsApi` | 凭据状态与安全读写 | status、set、read；UI 默认只拿 status |
| `ProviderApi` | ASR/LLM/Omni provider | 列表、验证、模型列表、激活项；实现必须由 Core `ProviderService` 提供，宿主只注入 credential/transport |
| `HistoryApi` | 历史与活动统计 | list/delete/clear/stats、录音导出 |
| `VocabularyApi` | 词典、纠正规则和建议 | list/add/remove/enable/accept/reject |
| `StylePackApi` | 风格包生命周期 | list/create/save/preview/activate/import/export |
| `LocalAsrApi` | 本地模型下载与运行时 | models、download、prepare、release、status |
| `SelectionApi` | 选区润色/选区语音 | capture、preview、confirm、cancel、revert |
| `QaApi` | QA 会话、录音和回答 | submit、sync、approve、cancel |
| `LessComputerApi` | Coding Agent 连续对话 | submit、cancel、dismiss、approve |
| `RemoteInputApi` | 远程输入服务器 | status、PIN、locale、local IP |
| `MarketplaceApi` | 市场与 GitHub OAuth | list/detail/install/upload/like/auth |
| `CodingAgentApi` | Coding Agent 检测、模型、风险、测试与审批 | detect、list models、risk、run/cancel、approve |
| `PlatformApi` | 能力和权限状态 | capabilities、microphone、accessibility、IME |
| `AuxiliaryApi` | 对既有文本/PCM 执行单轮共享处理 | repolish、retranscribe PCM、实际 ASR 归因、取消 |

每个接口只返回 core DTO 和 `BackendError`，不接受 Tauri 类型。Tauri command 和 egui 调用层分别把本宿主输入转换为这些 use-case 参数。

#### Less Computer 接口约定

`OpenLessBackend::submit_less_computer(transcript)` 是普通文本入口，Core 从同一份
preferences snapshot 解析 provider、可执行文件、model、permission mode、workdir、prompt
和护栏策略，并生成 Core-owned `SessionId`。需要把热键录音生命周期与 Agent 运行严格关联的宿主
先调用 `begin_less_computer_capture(session)` 预留实例级 capture lease，再使用
`submit_less_computer_with_session(session, transcript)`；其中 `session` 只用于取消和事件关联，
不允许宿主借此覆盖 Core 的 provider 或安全策略。宿主启动 recorder/ASR 失败、空转写或取消而
未进入 Agent run 时，必须调用 `abort_less_computer_capture(session)`；该方法对已提升为 run
的 session 是幂等 no-op。

提交前宿主可以显示自己的窗口或录音反馈；`less_computer_active_session()` 用于重连/诊断，
`less_computer_capture_cancelled(session)` 在宿主释放 capture lease 前报告取消。提交后只订阅
`BackendEventKind::LessComputerEvent`：

| 事件 | 语义 | UI 建议（egui 团队实现） |
| --- | --- | --- |
| `User { text, fresh }` | Core 已接受一轮输入；`fresh=true` 表示 dismiss 后的新会话 | 追加用户气泡并清理旧会话状态 |
| `Started` | provider 进程已开始 | 显示运行中 |
| `Delta { text }` | Agent 增量文本 | 追加到当前助手消息 |
| `Tool { name }` | provider 报告工具调用 | 显示工具活动，不执行工具 |
| `Compaction` | provider 压缩上下文 | 显示“整理上下文”状态（可选） |
| `Approval { token, command, reason }` | Core 等待一次高风险命令决定 | 仅展示 command/reason；通过 `CodingAgentApi::approve(token, bool)` 回传 |
| `Completed { text, cost_usd }` | 唯一成功终态 | 固化助手消息和费用（若有） |
| `Error { message }` | 唯一失败终态 | 显示可读错误和重试入口 |
| `Cancelled` | 唯一取消终态 | 清理运行态但保留已显示历史 |

事件中的 `seq` 由 backend 实例统一分配；UI 重连时先建立订阅，再调用 replay/snapshot，按
`seq` 去重。UI 不应自己维护 approval token、conversation flag、continuation history 或
provider 进程状态；这些均由 Core/Runtime Adapter 持有。未注入 `LessComputerRuntimeAdapter`
时，submit 必须返回 `BackendErrorCode::Unsupported`，不能伪造 `Completed`。

### 5.3 快照与事件

事件是跨两个宿主的真实接缝。核心事件必须表达“发生了什么”，而不是“哪个窗口要怎么显示”。建议定义：

```rust
pub struct BackendEvent {
    pub sequence: u64,
    pub session_id: Option<SessionId>,
    pub kind: BackendEventKind,
}

pub enum BackendEventKind {
    BackendStarted,
    BackendStopping,
    DictationStateChanged(DictationStateSnapshot),
    TranscriptDelta(TranscriptDelta),
    PolishDelta(PolishDelta),
    DictationCompleted(DictationResult),
    SelectionStateChanged(SelectionSnapshot),
    SelectionVoiceStateChanged(SelectionVoiceSnapshot),
    InsertFallback(InsertFallbackPayload),
    PreferencesChanged(PreferencesChange),
    CredentialsChanged(CredentialsStatus),
    HistoryChanged(HistoryChange),
    VocabularyChanged(VocabularyChange),
    StylePacksChanged(StylePackChange),
    DownloadProgress(DownloadProgress),
    PermissionChanged(PermissionSnapshot),
    HotkeyStatusChanged(HotkeyStatus),
    Notification(NotificationPayload),
    CodingAgentTest(CodingAgentStreamEvent),
    LessComputerEvent(LessComputerEvent),
    LocalAsrPrepareProgress(LocalAsrPrepareProgress),
    LocalAsrDownloadProgress(LocalAsrDownloadProgress),
    LocalAsrEngineChanged(LocalAsrRuntimeStatus),
    MicrophoneDevicesChanged,
    QaLevel(QaRecordingLevel),
    QaState(QaStateEvent),
    RemoteInputStatusChanged(RemoteInputRuntimeEvent),
    RemoteInputFailed(RemoteInputErrorEvent),
    VocabularySuggestionsChanged(Vec<PendingCorrection>),
}
```

事件约束：

- 每个 backend 实例的 `sequence` 单调递增；同一 session 的事件顺序可验证。
- 事件携带 `SessionId` 的地方必须由宿主丢弃过期 session，防止晚到结果污染新会话。
- 事件流是增量通知，不是唯一真相；收到丢失/滞后通知后，宿主重新读取 `snapshot()`。
- `subscribe()` 应使用可检测滞后的广播/订阅机制；订阅者落后时返回显式 `Lagged`，不能静默继续使用旧状态。
- 最终事件只发布一次；取消、失败和成功都必须有明确终态。
- 核心不发布 `capsule:state`、`chat-panel:shown` 等 Tauri 窗口事件名。Tauri adapter 可把 `DictationStateChanged` 映射为现有事件名，egui adapter 直接更新自己的 view model。

机器基线已把 30 个旧 Tauri event 全部分类为“core 语义事件映射”“纯 Tauri 窗口事件”或
“删除前需版本迁移”。以下 12 个原 `migrationRequired` 事件现已全部获得 typed core event，
并从业务模块的直接 emit 迁移到集中桥接：

1. `coding-agent:test`
2. `foundry-local-asr-prepare-progress`
3. `less-computer:event`
4. `local-asr:engine-changed`
5. `microphone:devices-changed`
6. `qa:level`
7. `qa:state`
8. `remote-input:error`
9. `remote-input:running`
10. `sherpa-onnx-asr-download-progress`
11. `sherpa-onnx-asr-prepare-progress`
12. `vocab:suggested`

这 12 项已按固定步骤完成迁移：在 core 定义稳定 DTO 和 11 个 `BackendEventKind` variant
（Foundry/Sherpa prepare 共用一个语义事件）；在 `tauri_events.rs` 集中映射为旧 React
payload；把领域实现的直接 emit 改为共享 `BackendEventPublisher`；纯窗口事件继续留在
Tauri；baseline/contract、serde fixture、30/30 分类完整性、Tauri mapping 和
secret-surface tests 同步更新。后续事件仍必须遵守同一流程。特别是 remote PIN、token 和
provider credential 不得进入 core event 或兼容 payload。

### 5.4 错误接口

现有很多 command 返回 `Result<_, String>`。核心应改用稳定错误类型，Tauri adapter 再把它序列化为兼容 JSON：

```rust
pub struct BackendError {
    pub code: BackendErrorCode,
    pub message: String,
    pub retryable: bool,
    pub details: Option<serde_json::Value>,
}

pub enum BackendErrorCode {
    InvalidArgument,
    InvalidState,
    Busy,
    Cancelled,
    PermissionDenied,
    Unsupported,
    Provider,
    Persistence,
    Platform,
    Internal,
}
```

约束：

- `code` 是机器可判断字段，`message` 是用户可读信息，不能让 UI 解析英文字符串。
- 凭据、token、PIN、Authorization header 和完整 provider 请求不能出现在 `message`、`details` 或日志中。
- 超时后的异步插入/提交必须表达“结果未知”状态，不能仅凭 timeout 自动重试导致重复插入。
- `Cancelled`、`Unsupported`、`PermissionDenied` 不能被 Tauri wrapper 统一转换为普通字符串失败。

### 5.5 BackendConfig 与依赖注入

`BackendConfig` 只包含配置值和路径，不包含窗口对象：

```rust
pub struct BackendConfig {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub resource_dir: Option<PathBuf>,
    pub home_dir: Option<PathBuf>,
    pub platform: PlatformCapabilities,
    pub locale: String,
}
```

`BackendDependencies` 通过 Interface 注入真正会变化或需要替身测试的依赖：

| Interface | 生产 Adapter | 测试 Adapter |
| --- | --- | --- |
| `TaskSpawner` | Tauri host / Linux Tokio runtime | 单线程 deterministic executor |
| `Clock` | 系统时钟 | 固定时钟 |
| `CredentialStore` | macOS/Windows/Linux/Android 安全存储 | 内存 vault |
| `TextInserter` | AX / TSF / fcitx5 / clipboard | 记录调用的 fake inserter |
| `AudioRecorder` | cpal + 平台设备 | PCM fixture recorder |
| `HotkeyController` | macOS/Windows/global-hotkey/fcitx5 | 可控 fake hotkey |
| `HostActions` | Tauri 窗口/托盘/系统操作 | 记录 action 的 headless host |
| `ResourceResolver` | Tauri resource dir / Linux 安装目录 | 临时目录 |

只在存在两个真实 Adapter 或一个真实 Adapter 加一个测试替身时建立 Interface；纯 Rust provider 和业务函数不为了形式而包一层。

### 5.6 HostActions

核心需要请求宿主执行窗口或系统动作时，使用语义 action，不接受窗口 label：

```rust
pub enum HostAction {
    ShowMain,
    FocusMain,
    ShowDictationFeedback,
    HideDictationFeedback,
    ShowSelectionPreview,
    ShowQa,
    ShowLessComputer,
    OpenExternalUrl(Url),
    OpenSystemSettings(SystemSettingsPage),
    RequestRestart,
    Notify(NotificationPayload),
}
```

Tauri adapter 把这些 action 翻译为 WebView/window 操作，Linux adapter 翻译为 egui 状态、窗口操作或 desktop integration。核心不决定实现方式，也不假设存在多个 WebView。

## 6. 状态、并发和生命周期契约

### 6.1 状态所有权

核心拥有：

- 当前 dictation / QA / selection session
- session phase、取消状态、插入结果和终态
- provider 选择、凭据状态、模型准备状态
- preferences、history、vocabulary、style pack 数据
- 下载进度、热键能力和权限快照

宿主 UI 拥有：

- 当前页面、tab、滚动位置和焦点
- 对话框展开与输入草稿
- 动画、过渡和 egui immediate-mode 临时状态
- 窗口大小、位置、装饰和前端布局

### 6.2 线程规则

- `update()`、Tauri command 入口和 egui frame 都不能执行阻塞的网络、磁盘、录音或模型加载。
- core 不调用 `tauri::async_runtime::spawn`；通过 `TaskSpawner` 或 core 统一的 Tokio runtime 执行后台任务。
- egui host 在 frame 中只 drain 非阻塞事件；收到事件后调用 `request_repaint()`，不能在 frame 内 `.block_on()`。
- Tauri event bridge 单独运行订阅任务，退出时先停止订阅，再关闭 backend。
- 所有可取消操作必须绑定 `CancellationToken` 或等价 session token；取消后仍可能晚到的结果必须被 session guard 丢弃。

### 6.3 初始化与关闭顺序

启动：

1. 宿主解析数据目录、资源目录、locale 和平台能力。
2. 宿主创建 platform adapters 和 `BackendDependencies`。
3. 构造 `OpenLessBackend`。
4. 注册事件订阅和 host action sink。
5. 调用 `backend.start()`，读取 `StartupSnapshot`。
6. 成功后启动全局热键、托盘 watcher、下载 watcher 等宿主任务。
7. UI 显示；未满足的能力通过 snapshot 明确降级。

关闭：

1. 禁止新 command/session。
2. 取消正在运行的会话和下载。
3. 停止热键、录音、设备 watcher 和远程 server。
4. flush 必须持久化的数据。
5. 停止 event bridge / runtime。
6. 调用 `backend.shutdown()` 并退出宿主。

## 7. 模块抽取与归属矩阵

这是迁移审计的初始归类；混合文件必须按函数拆分，不以文件名作为最终架构。

### 7.1 进入 `openless-core`

| 当前位置 | 迁移动作 |
| --- | --- |
| `src-tauri/src/types.rs` | 把领域 DTO、快照、枚举、serde 名称和校验迁入 core；平台专属类型另放 adapter module |
| `coordinator_state.rs` | 直接迁移 session phase、generation、终态和状态转换测试 |
| `correction.rs` / `edit_plan.rs` | 迁入 core 纯业务模块 |
| `endpoint_security.rs` | URL 格式校验和 DNS resolution/pinning 逻辑迁入 core；宿主只提供网络 executor |
| `selection_voice_intent.rs` | 意图分类、关键词和 JSON 解析迁入 core；UI 选择弹窗留在 host |
| `cli.rs` | CLI 参数解析和 `CliIntent` 作为纯输入类型迁入 core；进程激活/窗口操作留在 host |
| `persistence/` | 保留 store 逻辑；凭据底层改为 `CredentialStore` Interface；路径解析改为 `BackendConfig` |
| `asr/` | provider 协议、请求、响应、重试和 ASR 业务流程迁入 core；平台模型 runtime 由 adapter 注入 |
| `asr/local/` | 模型 catalog、选择和生命周期接口进入 core；Qwen3 MLX、Foundry、Sherpa、Whisper 等 native runtime 按 target 放入 platform adapter，避免 macOS vendored path 被所有 target 解析 |
| `polish/` / `llm_gemini.rs` / `net.rs` / `omni.rs` | 迁入 core；不得引用 Tauri window/event |
| `coordinator/{dictation,polish_flow,asr_wiring,resources,silence_auto_stop}.rs` | 保留业务流程，改用 event bus、宿主 Interfaces 和 session guard |
| `coding_agent/` | provider/model/权限/预算/路径/风险/版本/MCP 等跨宿主规则进入 core；进程、Git、临时配置、宿主审批和事件桥接留在 Adapter |
| 现有类型测试与状态机测试 | 测试随模块迁移，测试入口改为 core interface |

### 7.2 进入平台 adapter，但仍可被 core 注入

| 当前位置 | 目标职责 |
| --- | --- |
| `recorder.rs` | `AudioRecorder` 的桌面实现；设备枚举和 level monitor 的 host bridge 单独拆出 |
| `audio_mute.rs` | `AudioMuteGuard` 作为平台音频 Adapter；core 只依赖可选的 mute Interface |
| `hotkey.rs` / `global_hotkey_runtime.rs` | `HotkeyController` 的平台实现 |
| `qa_hotkey.rs` | QA 热键监听 adapter；QA session 状态和 use-case 逻辑进入 core |
| `combo_hotkey.rs` / `side_aware_combo.rs` / `shortcut_binding.rs` | 按平台保留实现，公共 binding 校验迁入 core |
| `insertion.rs` / `unicode_keystroke.rs` | `TextInserter` 实现；业务层只看 `InsertResult` |
| `linux_fcitx.rs` | DBus commit/hotkey 能力可作为 Linux adapter；资源目录检查和插件复制不能依赖 AppHandle |
| `windows_ime_ipc.rs` / `windows_ime_protocol.rs` / `windows_ime_session.rs` | Windows TSF IPC、协议和 session adapter；公共状态类型留在 core |
| `windows_ime_restore.rs` / `windows_ime_profile.rs` | Windows IME 恢复、键盘列表和注册表 adapter；公共设置 patch 留在 core |
| `host_document/` | macOS host document adapter；core 只接收 optional context |
| `permissions.rs` | capability/permission Interface；系统设置打开动作属于 host |
| `device_watch.rs` | OS 设备 watcher；通过 `BackendEvent` 或 host callback 回报 |
| `remote_server/` | 协议和业务可进 core；监听生命周期、端口、资源路径由 host 注入 |
| `external_url.rs` | URL scheme 校验可进 core；实际打开浏览器/Android Intent 必须由 host 实现 |

### 7.3 只进入 Tauri adapter

| 当前位置/内容 | 处理 |
| --- | --- |
| `lib.rs` 的 `tauri::Builder`、plugin 初始化、`generate_handler!` | 移入 Tauri host |
| `commands/` 中 `#[tauri::command]` 函数 | 保留为薄转换层，只做参数解析、core 调用和错误序列化 |
| `AppHandle` / `Window` / `WebviewWindow` 操作 | 移入 `tauri_events.rs` 和 `host/` |
| tray menu、窗口创建/显示/隐藏/定位、vibrancy/Mica、single-instance | 移入 Tauri host |
| Tauri updater/dialog/shell/autostart/fs plugin | 只在对应 host 使用 |
| `tauri.conf.json`、capabilities、Tauri resources | 只服务 Tauri desktop/mobile |
| `mobile_runtime.rs` / `android/` | 继续作为 Android Tauri host/JNI/overlay/IME adapter，不进入 Linux package |

### 7.4 Android 处理

- 保留现有 `#[cfg(mobile)]` 分支和 Android Tauri host。
- 将 Android 业务可复用部分依赖 core；JNI、overlay、IME、Keystore 留在 Android adapter。
- 不让 Linux egui 为 Android 的 unavailable 能力增加条件分支。
- `PlatformCapabilities` 由 core 定义结构，宿主提供真实值；Android 继续返回当前约定。
- `build_target.rs` 只保留为构建目标/`cfg` 的测试辅助；不得成为运行时模块或 core 的宿主依赖。

## 8. 详细实施步骤

### 8.1 执行顺序、责任和阻塞关系

里程碑按下表推进。除 M7 的 Interface 移交外，egui 团队不阻塞共享后端迁移；他们可以在 M7
交付后基于 fake/headless Adapter 并行开发 UI。任何阶段都不能以复制业务规则到宿主来绕过前置项。

| 里程碑 | 主责 | 依赖 | 退出后解锁 |
| --- | --- | --- | --- |
| M0 决策与基线 | 架构/后端负责人 | 无 | 固定平台范围、兼容基线和版本规则 |
| M1 package 骨架 | 构建负责人 + core 负责人 | M0 | core/Linux 可独立解析和编译 |
| M2 类型与错误 | core 负责人 | M1 | 两个 Adapter 可共享 DTO、错误和能力语义 |
| M3 依赖注入与生命周期 | core 负责人 + 平台负责人 | M2 | 可用 fake Adapter 做 headless 测试 |
| M4 Coordinator 与事件 | core 负责人 | M3 | 两个宿主可消费同一状态机和语义事件 |
| M5 领域迁移 | 各领域后端负责人 | M2–M4 | 业务规则只有 core 一份实现 |
| M6 Tauri Adapter | Tauri 负责人 | 对应 M5 领域逐项完成 | React IPC 保持兼容且不再承载业务规则 |
| M7 egui Interface 移交 | core/Linux host 负责人 | M2–M4 的稳定 Interface；允许以 Unsupported 标记未接线能力 | egui 团队可独立开发 view model/UI |
| M8 Linux 非 UI Adapter | Linux host 负责人 | M3、M4、对应 M5 领域 | Linux 宿主可调用真实共享主链路 |
| M9 测试与质量门禁 | 测试/构建负责人 | M4–M8 逐项接入 | 合并与发布候选具备可重复证据 |
| M10 打包与发布 | 发布负责人 | M8、M9；真实 egui 入口由 UI 团队交付 | Linux 原生产物可独立发布 |

交付发生变化时，主责方必须同步更新 Interface contract、fixture、迁移说明和对应 Adapter
contract tests；仅更新实现代码不能视为完成。

### 8.2 当前工作树的剩余关键路径

以下顺序是从当前实现推进到最终验收的唯一关键路径。每一步完成后先过本步门禁，再进入
下一步；不得通过在某个宿主复制业务判断来绕过未完成的 core 工作。

1. **已完成：冻结每会话配置快照。** core 已定义 `DictationContext`，在
   `start_dictation()` 时一次性固定麦克风、ASR/LLM/Omni channel、模型、语言、翻译目标、
   ASR prompt、风格包/润色 prompt、流式插入和 fallback 策略。会话开始后修改设置只能影响
   下一会话，不能让正在运行的 provider 读取到一半新一半旧的偏好。
2. **已完成：让 Pipeline 消费会话快照。** `DictationEngine`、`AudioRecorder`、
   `TranscriptionEngine` 和 `TextPolisher` 的最小参数，使 recorder 选择设备、provider 选择、
   ASR prompt 与 polish prompt 都来自同一个快照；补设置并发修改、取消和迟到结果测试。
3. **已完成：修复 provider router 的会话占用语义。** `DictationEngineRouter::start()` 使用
   `HashMap::entry` 原子占位；回归测试证明第二次 start 返回 `Busy` 后，原 session 仍由
   第一次选中的 Adapter 完成，不能被新 Adapter 接管。
4. **已完成：共享 provider registry、生产 factory 与 Provider 管理面。** core 已提供按会话固定 Adapter 的
   `TranscriptionRouter`、`TextPolisherRouter` 和 `DictationEngineRouter`，并覆盖设置切换后
   旧 session 不漂移、缺失 provider 显式 `Unsupported`、traditional/Omni 分流测试。云/实时 ASR、
   OpenAI-compatible/Gemini/Codex LLM、Omni、Auxiliary 和 QA provider Implementation 现由 core
   持有，包含 credential account、默认 endpoint/model、协议选择、取消、流式输出和 session
   占用语义。Tauri 注册共享实现并追加 native/local ASR；Linux 通过
   `LinuxBackendBuilder::from_shared_providers(config)` 注册同一批共享实现，不读取 Tauri Adapter。
   channel ID、协议类型和模型在会话开始时分别冻结；Omni 的 API key、endpoint、model、extra
   headers 和 temperature 全部按冻结的 provider ID 读取，不依赖运行中的 active provider；重复
   session 不会覆盖原 cancellation route。`ProviderService::validate/list_models` 已迁入 Core，按
   channel-scoped credential 解析 provider/type/model；Tauri command 只做参数和旧错误转换，Linux
   `from_shared_providers` 注入同一 service。静态清单、OpenAI/Gemini 响应解析、Omni channel 拒绝、
   错误脱敏和 Linux 非 `Unsupported` factory contract 已覆盖。
5. **已完成：迁移完整润色 prompt 语义。** 旧 `polish/prompt_compose.rs` 的 XML envelope、输入净化、
   prompt injection 防护、前台应用、光标上下文、历史 turns、翻译规则和 user prompt envelope
   已移入 core，并由 3 项固定 prompt contract 和 core/Tauri provider tests 覆盖。
6. **已完成：为 Tauri 构造真实 core Pipeline。** 录音、凭据、host action，以及按 session 执行
   `prepare/insert/cancel` 的 Windows TSF/SendInput/Paste、Android strategy 与 macOS 插入 Adapter
   已接入；TSF 派发后的超时被保留为 outcome-unknown，禁止触发可能重复落字的 fallback。
   setup 构造唯一的 `Arc<OpenLessBackend>`，Tauri 与 compatibility Coordinator 共享同一组
   repository；生产 provider Adapter 也已接入该 Pipeline。
7. **进行中：逐入口切换 Tauri 听写主链。** React dictation start/stop/cancel command、CLI
   toggle/cancel、Android JNI、remote WebSocket，以及桌面普通听写热键的
   Pressed/Released/Combined 和 Esc 取消已进入同一个 facade。Android 通过
   `DictationStopOptions` 保留“stop 时决定 translation”的既有语义，同时只更新冻结快照中允许
   变化的翻译开关；remote 使用 16 kHz、单声道、signed Int16LE external PCM seam，session ID
   严格关联，stop/cancel 后拒绝迟到帧。桌面宿主继续拥有 QA panel 优先分流、shortcut
   recording、modifier-only combo arbitration、debounce/cooldown 和物理 listener/window
   fallback；这些宿主机制不能重新拥有听写 session 状态。静音自动停止和 Starting pending stop
   仍调用 Coordinator 旧 `end_session`，必须与 Less Computer 语音生命周期一起迁移后，才能
   把桌面热键主链标为全链路完成。托盘审计未发现听写 start/stop 入口。
   TLS/PIN/WebSocket 与 Android overlay/IME 继续留在 Tauri Adapter。其余 Coordinator 入口必须
   按行为 contract 逐项切换，不能通过整体代理改变产品行为。
8. **已完成：集中完整 legacy event mapping。** 机器基线中的 30 个 legacy event 已逐项标注为
   “core 语义事件映射”或“纯 Tauri 窗口事件”，并通过完整性检查（30/30、无重复/遗漏）；原
   `migrationRequired` 分类已清空。12 个旧事件由 11 个 typed core event 覆盖，统一在
   `tauri_events.rs` 映射，业务模块不再直接发射这些事件。后续禁止新增业务直接 emit 点。
9. **进行中：迁移复杂领域 Implementation。** Coding Agent 的跨宿主规则和 DTO 已进入 core，
   Tauri command 已收敛为授权/兼容转换层，真实进程、Git、临时文件和事件转发由
   `TauriCodingAgentApi` 负责。Local ASR 的 catalog、设置事务、运行时生命周期 Interface、
   Core Implementation、engine-changed 事件语义与 Generic/Foundry/Sherpa Tauri command 薄包装
   已经落地；`TauriLocalAsrRuntimeAdapter` 直接使用共享 preferences repository 与 Qwen/Whisper
   cache，不再通过 `AppHandle` 回取 Coordinator；完整本地门禁已通过，剩余工作是各原生 runtime
   证据。Marketplace/GitHub OAuth 的 HTTP、
   认证、归档、安装、upload、device-flow 状态机已经进入 core，Tauri command 已只保留参数、旧
   wire/error 转换与最终文件写入；该领域的 core contract 17 项和严格 clippy 已通过。Selection
   Core 的 17 项 contract 与 Selection Voice 的 13 项 contract 已完成，生产构造已注入新的 Tauri
   runtime，旧 Coordinator wrapper、正式热键/command 与安全 revert 路径已经收口；QA Core、
   `TauriQaRuntimeAdapter` 生产接线和 Remote Input Core 也已建立；Linux 生产 factory 已自动注入
   Core `MarketplaceApi`，`LinuxHost::download_marketplace_archive` 只把 Core 校验后的归档写入用户
   选择的绝对路径，使用 create-new 语义拒绝覆盖并在写入失败时清理不完整文件；QA/Remote lagged resync、
   Remote secret wire、Remote WebSocket 单 stream/restart stale-lease 和 Less Computer
   listener-first replay/pending/dedup/truncation contract 已补齐；历史重润色、手工重转写和静默
   重试已统一进入 `AuxiliaryApi`，repolish 只冻结 LLM/Omni，retranscription 只冻结 ASR，并由
   Transcription Adapter 报告默认值解析后的实际 provider/model。云 ASR/LLM/Omni/Auxiliary/QA
   协议构造、凭据路由和取消已统一进入 core；Selection Voice 的 correction、instruction polish、
   自动 intent model/fallback、delivery decision、EditPlan、translation 和 QA preview revision 也已
   进入 Core 高层 use-case，Tauri 只保留录音/窗口/热键/opaque insertion target 与 apply outcome。
   provider 验证/模型列表已由 Core `ProviderService` 统一，Tauri command module 的协议请求副本
   已删除并由 source contract 守护。剩余重点是旧 Coordinator 其他宿主耦合审计与原生平台证明。每个 Tauri
   command 只做参数/DTO/错误转换，Linux 未提供的平台能力由真实 Adapter 或稳定 `Unsupported`
   表达。逐项步骤见 8.3 节。
10. **进行中：收窄旧 Coordinator 的兼容宿主职责。** 已完成的宿主隔离包括：`Inner.app` 已替换为
    显式 `TauriCoordinatorHost`；`bind_app(AppHandle)` 已删除；`Inner` 与 `capsule_focus` 已恢复
    module 私有；Coordinator/capsule 子模块中的 `AppHandle`、`WebviewWindow`、直接 `emit*` 和
    `tauri::async_runtime::{spawn,spawn_blocking,block_on}` 均已清零；Sherpa/remote 等业务事件经
    typed Core event 与 `tauri_events.rs` 集中映射。capsule layout 去重、cursor passthrough、style、
    fallback card、presentation generation、deferred payload 以及 show/hide/no-activate 和
    macOS/Windows 原生窗口行为均由 Host 持有；`TauriCapsuleWindow::apply_capsule_payload` 只接收
    payload、显示决策、style 和 Space reassert 窄值，不再回调整个 `Inner`。此外，
    `core_adapters.rs` 的 `managed_coordinator` 反向查询已删除；hotkey status 与 QA 可见性由构造层
    创建的窄共享状态分别注入 Coordinator/Adapter，Local ASR 共享同一 repository/native cache。
    Selection Voice 本批已删除 Tauri 中的 correction、prompt、自动分类、EditPlan、translation 和
    output-mode 分支，QA Adapter 直接调用 Core `edit_preview` 后只绑定平台 target；相应 source
    contract 会阻止这些业务 token 回流。
    Less Computer 的生产热键按下现在先调用 Core capture lease，再由
    `coordinator/hotkey_loops.rs` 进入 `begin_session_as_with_session_id`；松开、静音自动停止和
    Starting pending stop 仍调用兼容层 `end_session`，但 Core 通过同一 session id 接管提交、取消
    和 Agent 终态。Coordinator 中剩余的 `state` 字段只表达宿主录音/热键生命周期，不能被 Linux
    egui 读取或当作业务 API。下一步按生产调用图逐项分类剩余 Coordinator 方法：纯 Host 生命周期、
    授权、wire 转换和 socket/native runtime 留在 Tauri Adapter；跨宿主业务状态、provider 协议和
    设置/热键事务迁入 core；仅由旧测试引用且无生产消费者的 wrapper 删除。settings/hotkey 的“兼容化解/校验 → 生成显式 effect plan → 平台
    prepare/commit → 单次持久化/事件 → receipt 逆序补偿”事务已经迁入 core；Tauri/Linux Adapter
    不再回读偏好文档来猜 listener 目标，style-pack 删除也直接消费 Core 专用 outcome。完成每批迁移后重跑 command/event baseline、
    source contract、Tauri 全量测试和残余引用检查。窗口、托盘、updater、dialog、shell、autostart、
    single-instance、Android JNI/overlay/IME 与 native ASR runtime 始终留在宿主，不为形式共享塞入 core。
    还必须完成 runtime seam 审计：core provider transport 可以使用宿主已经启动的 Tokio runtime，
    但生产路径不得在无 runtime 时自行 `Runtime::new()`；应改为注入的 runtime/task spawner 或
    明确要求由异步宿主调用，并增加 headless no-private-runtime contract。`rg` 对
    `tokio::spawn`、`Handle::current`、`Runtime::new` 的结果要逐项标注为“宿主 runtime 内运行”或
    “测试专用”，未标注项不能进入 M9 完成状态。
11. **已完成 Linux 非 UI runtime 接线；真实原生确认待 runner。** `SelectionPolishEvent` 已调用共享
    `SelectionApi`；空闲态先收到的 `TranslationModifierEvent` 已作为下一次 dictation press 的
    `DictationStartOptions`，不会修改已经冻结的活动 session；`LinuxNativeRuntime` 已统一拥有
    primary broker、hotkey listener、错误 drain 和 shutdown/join。下一步在真实 fcitx5 上记录
    translation 与 dictation 信号顺序：若 translation 可能后到，必须先冻结关联规则并补事件
    时间线测试，不能靠修改活动 session 猜测用户意图。
12. **已完成（Interface）：冻结 1.x egui 接口交付。** contract、公开 re-export、完整 headless example、
    能力 fixtures、view-model 映射和 `AuxiliaryApi` 单轮处理/取消/归因契约已更新；egui 组可以
    只依赖 facade/DTO/event/fixture 并行开发 UI，不读取 core 私有模块，也不等待 M10 正式打包。
    设置/快捷键 DTO、`LinuxHost::save_settings` reconcile 入口、`update_settings_strict` 严格拒绝入口、
    snapshot revision 和显式 Linux effect target 已进入公共 contract；egui 组不能直接调用底层
    `set_preferences*` 绕过事务。Selection/Selection Voice 的 preview、confirm、cancel、stale、
    outcome-unknown 与 Linux preview/revert `Unsupported` 已由 fixture、headless 示例和第 4 项
    host contract 覆盖；当前公共面/受影响门禁已经重跑通过。
13. **已完成：消除测试旁路。** 已删除 `backend-tests/tests/backend_rust.rs` 的 `#[path]`
    include 与 Tauri stub；`backend-tests` 现在只直接依赖公开 `openless-core` 并运行一项 core
   contract。原 118 项测试由 Tauri crate 自身的
   `cargo test --locked --manifest-path "src-tauri/Cargo.toml" --lib` 承担，不再复制源码；capsule
   Host 收口前的历史 Windows 基线为 1080 passed、7 ignored、0 failed；该数字已由下一步的最新
   工作树结果取代。
14. **已完成当前 Windows 本地重验；原生 runner 仍由下一步单独验收。** capsule Host 收口之前，Windows 本地的
   frontend build/58 项 tests、workspace fmt、Core 261 项 unit 与 75 项领域 contract、Linux 22 项
   crate 与 2 项 host contract、公开 Core compatibility 1 项、Core/Linux 严格 clippy、Tauri
   `cargo check --lib`、Tauri 1080 passed/7 ignored 的 `cargo test --lib`、command/event baseline、
   依赖方向、secret surface、测试隔离和公共面门禁均通过。最新 capsule 增量已通过
   `shared-backend-wire-contract`、macOS capsule Spaces、Windows UI config 三个源码契约、Tauri
   `cargo check --locked --lib` 和 15 项 `capsule_` 定向测试；当时 check 报告 280 项既有/迁移期 warning，
    warning 数不作为成功证明。共享 provider 抽取后的当时工作树已通过 Core 567 unit + 75
    integration contract、Linux 25 crate + 3 host contract、Core/Linux 严格 clippy、Tauri
    `cargo check --locked --lib`（compiler summary 276 项 warning）和 778 passed/0 failed/7 ignored；
    frontend build/58 tests、公开 Core compatibility 1、fmt、196/30/29 基线、依赖方向、秘密面、
    测试隔离、Linux 公共面和 diff hygiene 也曾在同一工作树通过。此后删除了 Tauri Adapter 中
    永久禁用的 legacy 云 ASR/润色/Omni provider 副本，并加强 source contract；因此 source
   contract、workspace fmt、Tauri check/test 必须重新运行。该轮后续工作树已通过 frontend build/58
   tests、Core 596 unit + 79 integration contract、Linux 30 crate + 4 host contract，另有 3 个显式
   ignored native contract、公开 Core compatibility 1、Core/Linux 严格 clippy、Tauri check 与 730
   passed/0 failed/7 ignored 的 macOS Tauri suite；最终证据以 fork CI run 33408317390 为准。fmt、
    196/30/29 基线、依赖方向、秘密面、测试隔离、Linux 公共面、source contract、headless example
    和 tracked diff hygiene 也在该 CI run 通过。该本地证据不替代第 15 步的原生 runner 结果。
15. **取得原生 CI 证据。** Ubuntu 验证 dbus/keyring/cpal/fcitx5、Linux host 和无 WebKitGTK
    依赖；macOS/Windows 验证 Tauri adapter；Android 验证 mobile target/JNI/Gradle。任何缺失的
    runner 证据保持未完成，不能由 Windows cross-target check 推断。
16. **验证 Linux 打包。** 在 Ubuntu runner 构建 fcitx5 plugin 与 release binary，生成 deb、
    rpm、AppImage，检查 desktop/AppStream metadata、ELF `ldd`、包内路径、AppImage 解包内容、
    单实例协议、资源解析、SHA-256、minisign 和独立 updater manifest。
17. **解除发布门禁。** 只有 egui 团队替换 `main.rs` UI stub、UI 验收完成、签名 secret 可用、
    M9/M10 原生证据全部通过后，才允许 Linux workflow 响应 release tag；在此之前只允许
    `workflow_dispatch`/`workflow_call` 生成验证产物。
18. **最终文档与删除审计。** 更新 README/RELEASING/contract/迁移说明和 M0 baseline；用
    `rg` 确认 core 无 Tauri/egui、Linux 无 Tauri/WebKitGTK、Tauri 业务模块无遗留直接 emit，
    最后逐项勾选第 12 节，不用“整体看起来可用”代替逐项证据。

### 8.3 剩余复杂领域的逐项执行清单

本节是 8.2 第 9–12 步的可执行展开。每个领域都遵循同一顺序：先冻结 Interface 和 observable
contract，再把业务 Implementation 放进 core，随后实现平台 Adapter，最后切薄 Tauri command。
不能先让 command 代理旧 Coordinator，再把代理层称为共享实现。

#### 8.3.1 设置与热键事务收口

设置和快捷键不是单纯的 JSON 持久化：一次保存可能同时改变 legacy 字段、快捷键冲突关系、
原生 listener、活动 ASR provider 的安全存储映射和 Windows 键盘列表。业务规则必须归 core，
平台调用必须归 Adapter，而“全部成功或按既定策略恢复一致状态”的事务语义也必须只有一份。

**当前已完成**

1. `openless-core::shortcut_types` 已拥有快捷键字符/修饰键语法、左右修饰键限制、物理重叠判定、
   legacy trigger 转换、dictation legacy 字段同步，以及 dictation/translation/QA/style/open-app/
   Selection/Coding Agent/style-pack 之间的冲突规则。
2. Core 已提供 `SettingsCollisionPolicy`、`SettingsUpdateOptions`、`expected_preferences_revision`、
   strict/reconcile、preserve-style、legacy 同步、typed effect plan、typed receipt/failure/outcome 和
   单写入 gate。stale revision 在运行平台副作用前稳定返回可重试 `Busy`。
3. Tauri `shortcut_binding` 只保留 `ShortcutBinding -> global_hotkey::HotKey` 的原生转换；mobile
   stub 复用 core 语义校验，但原生解析继续显式返回 mobile unavailable。
4. `OpenLessBackend::update_settings` 已成为 core-owned transaction use-case：先 prepare/commit
   平台 effect，再只持久化一次、发布一次；prepare、commit 或 persistence 失败时按 receipt 逆序
   restore，补偿错误与主错误结构化返回，不会把部分成功伪装成成功。
5. `commands/settings.rs::reconcile_hotkey_collisions` 的“核心 dictation 优先、非核心键按优先级
   恢复旧值或停用、translation 必须回退默认值、style-pack hotkey 最低优先级”的产品规则迁入
   core。整表 settings 保存可以按既有 #904 兼容策略自动化解；单项快捷键命令仍应对冲突直接
   拒绝，两个入口保持不同的既有产品语义。
6. 已定义最小 `HotkeyRuntime` Interface。输入是 core 计算出的完整目标 binding set/diff，输出是可供
   补偿的 typed receipt；Adapter 不得反向读取已保存 preferences，也不得接收 `Coordinator`、
   `AppHandle` 或窗口 label。Tauri 实现注册 global-hotkey/combo/side-aware listener，Linux 实现
   fcitx5/DBus listener，测试实现记录 apply/restore 顺序。
7. 设置事务严格执行以下顺序：

   1. 读取并规范化 `previous`，生成经校验或兼容化解后的 `next`；
   2. 计算 hotkey、活动 ASR provider 和平台设置的 typed effect plan；
   3. 让对应 Adapter 以显式 `next` 执行可失败副作用，不允许 Adapter 从全局状态猜目标值；
   4. 全部副作用成功后只持久化一次 `next`，再发布一次变更事件；
   5. 任一步失败时按逆序补偿到 `previous`；补偿失败时返回包含主错误和补偿错误的结构化失败，
      按现有一致性策略决定恢复旧状态或 roll-forward，绝不能返回假成功或留下无报告的分叉；
   6. 整个事务使用单写入 gate，拒绝并发设置保存相互覆盖。

   活动 ASR provider 继续通过安全存储 Interface 同步；Windows 键盘列表只由 Windows Adapter
   执行。不要为了复用而把 Windows 注册表、global-hotkey 或 fcitx5 类型放进 core。
8. 整表 `persist_settings`、dictation/translation/QA/switch-style/open-app/selection/Coding Agent/
   combo/style-pack 快捷键生产入口已切换到该 use-case。command 内旧 settings/hotkey 事务副本及
   previous/write/refresh/rollback helper 已删除；style-pack 删除通过 Core 专用 outcome 返回显式
   hotkey effect。
9. Linux 公共面只暴露携带 snapshot revision 的 `LinuxHost::save_settings`（reconcile +
   preserve-style）和 `update_settings_strict`；合法保存、冲突拒绝、active provider、stale revision、
   effect compensation 与稳定 `Unsupported` 已由 3 项 host contract 覆盖。
10. Core 成功/失败矩阵已覆盖校验失败、持久化失败、runtime prepare/commit 失败、补偿失败、
   并发/stale revision、一次持久化/一次事件和 preserve-style；Linux runtime 覆盖 receipt 逆序恢复。

11. `legacy-preferences-write` feature 与 `OpenLessBackend::set_preferences*` 公共兼容面已删除；四个
   旧 writer 仅以 core crate 内 `#[cfg(test)] pub(crate)` helper 存在，Tauri/Linux 宿主无法启用或
   调用该旁路。公共面门禁同时拒绝 feature 回归与重新出现 `pub fn` writer。

**仍需完成（原生 runner）**

1. 在对应原生 runner 完成跨宿主失败矩阵：原生注册失败、ASR vault 同步失败、Windows
   keyboard apply 失败、第一次补偿失败、listener restore 失败、并发写入。每项都断言最终偏好、
   原生 listener、revision、事件数和错误码；mobile/不支持能力必须稳定返回 `Unsupported`。
2. **本地已完成，原生 runner 待完成。** settings/hotkeys/QA、Linux public-surface/host contract、
   Tauri 全量 suite 和 frontend compatibility 已重跑；残余引用确认旧 write/refresh/rollback 编排
   与 Tauri 事务 helper 均已删除，listener runtime 不再从 preferences 反推 target。
   Android/macOS/Ubuntu 的原生失败矩阵仍按上一项保持未完成。

**退出条件**

- 快捷键语法、冲突、兼容化解和设置事务只有 core 一份 Implementation。
- Tauri/Linux Adapter 只执行显式 effect plan，并能以 receipt 恢复；不读取或修改业务偏好。
- React 旧 command/字段/错误兼容不变，egui 只依赖 validated Interface 即可获得同样规则。
- 成功只产生一次持久化与一次语义事件；任何失败都有可测试的一致最终状态和明确错误。

#### 8.3.2 Local ASR 收口

**当前已完成**

1. core 已定义 Generic、Foundry、Sherpa ONNX 的统一 runtime/target/mirror、catalog、settings、
   status、remote info、model card 和 model test DTO。
2. `LocalAsrService` 已拥有设置校验、模型选择、镜像、语言、keep-loaded、存储迁移和运行时
   生命周期的业务语义；原生模型引擎、下载和文件操作通过 `LocalAsrRuntimeAdapter` 注入。
3. 三组 Tauri command 已只调用 `BackendServices.local_asr`，只保留旧参数和 React wire DTO
   转换；Generic 下载进度已改由 typed core event 进入集中事件桥接。
4. 成功的 runtime mutation 会读取并发布最新 `LocalAsrRuntimeStatus`；失败操作不发布伪造的
   成功状态。`set_active_model`、`set_foundry_runtime_source`、`set_keep_loaded_secs`、`prepare`、
   `release` 和 `delete_model` 已统一该语义。
5. Sherpa core model 到旧 wire DTO 的转换已改为 `TryFrom`；未知 family/mode 返回错误，不再
   `panic!`。
6. 定向证据：core `local_asr_contract` 6 项、Tauri `wire_contract_tests` 4 项通过；Local ASR
   接线后的 Tauri `cargo check --lib` 已通过。
7. `TauriLocalAsrRuntimeAdapter` 已直接注入共享 preferences repository 与
   `TauriNativeAsrDependencies`；storage/status/release/preload/delete/test 不再回取 Coordinator，
   非 Windows Coordinator 与 Core native ASR 使用同一 Qwen/Whisper cache。
8. 格式检查、完整 frontend 58 项、Tauri wire contract、Tauri `cargo check --locked --lib` 与
   `cargo test --locked --lib` 已通过；旧 command 名、camelCase/nullable/error 字段由源码契约和
   完整 suite 共同守护。

**剩余步骤**

1. 在对应原生 runner 验证 Generic、Foundry、Sherpa runtime 的准备、释放、取消和 engine-changed
   事件；Windows 上的 fake/contract 不能替代 macOS/Linux/Android 的原生能力证明。

**退出条件**

- Local ASR command 不再直接读取 Coordinator、native runtime `State`、下载 manager 或偏好 store。
- core contract、Tauri wire contract、typed event mapping 和完整本地门禁全部通过。
- Linux 未提供某个 native runtime 时返回 `Unsupported`，不引用 Tauri runtime 作为替代。

#### 8.3.3 Marketplace 与 GitHub OAuth

Marketplace 的 HTTP、OAuth、归档校验和安装事务属于跨宿主业务规则，现已形成深的 core
Module；文件选择器、目标路径授权和 Android `content://` 最终写入仍属于宿主能力。

**当前已完成**

1. `MarketplaceUploadResult`、`MarketplaceLikeResult`、`MarketplaceMyPackItem` 和 tagged
   `OAuthPollResult` 已进入 core，并有稳定 host-facing JSON fixture。
2. `MarketplaceApi` 已表达结构化 upload/like/my-packs/OAuth poll 结果；`download_archive` 返回由
   core 下载并验证的 bytes，最终 filesystem 或 Android `content://` 写入归宿主。
3. `MarketplaceConfig` 和构造接线已进入 `BackendDependencies`；公共请求使用匿名 client 且绝不
   附加 bearer，匿名与认证 client 都拒绝 redirect，认证 redirect 不会访问目标地址。
4. `list/detail/install/download_archive/upload/toggle_like/delete/my_likes/my_packs/auth_status` 以及
   device-flow 的 start/poll/cancel/logout 均由 Core Implementation 提供，不再由 Tauri command
   持有 HTTP 或认证状态机。
5. 通过注入的 `CredentialStore` 读写 GitHub token；401 会先设置 backend 实例内 tombstone，再
   尝试持久删除。即使删除失败，认证状态也立即变为 signed-out，后续请求不会再次发送旧 token。
6. core 同时检查 declared `Content-Length` 与 streamed bytes 上限，下载后执行 ZIP 校验；实例级
   `try_lock` 保证并发 install 在第二次请求出网前返回 `Busy`。
7. 安装使用 `StylePackStore::import_from_zip_bytes_with_origin` 原子提交 pack/origin，成功后 revision
   只增加一次并发布一次 `StylePacksChanged`；失败不留下 pack、revision 或成功事件。
8. upload 直接复用 core ZIP export 生成 multipart；首次上传成功后把 remote ID/login 写回本地
   origin，并沿用 style-pack revision/event 语义。
9. device-flow registry 已收进 backend 实例，拥有 generation、start 竞态失效、cancel、expiry、
   poll interval、`slow_down`、in-flight cancellation guard 和单次 token consumption；token 保存前
   会再次核对 lease。device code、access token、Authorization header 不进入 Debug、日志、event、
   error details 或普通 DTO。
10. Tauri Marketplace/OAuth command 已只做参数转换、core 调用及旧 React wire/error 转换；归档
    下载后的 filesystem 或 Android `content://` 写入仍留在宿主。旧 command 的全局 lock、HTTP
    helper、OAuth registry 和 ZIP 业务逻辑已删除。
11. 最新工作树已完整运行 `marketplace_contract`，17 项全部通过；
    `cargo clippy --locked -p openless-core --all-targets -- -D warnings` 通过。
12. Tauri Marketplace host sink 2 项与 GitHub OAuth wire 2 项通过；残余引用检查未发现 command
    中保留 HTTP、token vault、ZIP validation、全局 install lock 或 device-flow registry。

**剩余验证步骤**

1. **已完成（Windows 本地）**：完整 Tauri `--lib`、frontend contract 和第 12 节可在本机执行的
   全量门禁已经重跑；结果见第 12.4 节。该证据只证明当前 Windows 工作树，不能替代下列 Linux、
   Android 和 macOS 原生 Adapter 验证。
2. **已完成（Linux Interface）**：生产 factory 通过 Secret Service credential Adapter 使用同一
   `MarketplaceApi`，`LinuxHost::download_marketplace_archive` 提供 filesystem archive sink；egui
   只接触 Interface/DTO，不接触 token、HTTP client 或 URI 解析。
3. 在 Android/macOS/Ubuntu 原生 runner 验证各自 credential、文件授权和最终归档写入 Adapter；
   Windows contract 不能替代这些平台证据。

**退出条件**

- Marketplace 业务规则只有 core 一份 Implementation，宿主只处理平台授权、wire 转换和最终写入。
- public/auth/401/archive/install/upload/OAuth/secret-surface contract 在同一最新工作树上全部通过。
- React 字段、tagged union 与错误兼容测试通过；Linux 未接线的宿主能力明确返回 `Unsupported`。

#### 8.3.4 Selection polish 与 selection voice

**当前已完成**

1. `SelectionCapture`、`SelectionRuntimeAdapter`、`SelectionPhase`、`SelectionSnapshot` 和公开的
   `SelectionPolishOutputMode` 已成为 core Interface；窗口 label 与平台选区句柄没有进入 core。
2. `SelectionService` 已拥有 preview、session-scoped confirm、direct apply、cancel、completed
   replacement 单次 revert、重复 begin 的 `Busy`、generation guard、迟到 provider 结果丢弃和
   `Completed/Cancelled/Failed` 单次终态。
3. Selection 与听写复用同一 provider resolution 和 `TextPolisherRouter`；每个 session 冻结 LLM
   channel/provider type/model，以及 capture-time `front_app`，不读取 cursor context、dictation
   history 或 ASR prompt。
4. 成功 direct replacement 会写入 Selection history 并统计 vocabulary hits；失败会释放平台
   target；apply 的 `OutcomeUnknown` 会进入可见 snapshot 且绝不自动重试。
5. `SelectionStateChanged` 已是 typed core event。当前工作树的 `selection_contract` 17 项整体通过，
   覆盖 preview/confirm、显示/隐藏事件顺序、shutdown、outcome-unknown、history、vocabulary、最终
   纠正、Raw passthrough、防注入 instruction envelope、activity/timing attribution、provider/context
   冻结、单次安全 revert、cancel/Busy/迟到结果和 provider failure。
6. Tauri 生产构造已注入 `TauriSelectionRuntime` 和共享 polisher；runtime 按 `SessionId` 保存
   `SelectionInsertionTarget`，区分 preview/direct apply，并把平台插入结果映射为 `InsertOutcome`。
7. `TauriSelectionRuntime` Adapter contract 已覆盖 target 注册、preview 目标恢复、stale/cancel、重复
   capture 和单次安全 revert；切换窗口、session 过期、重复 revert 或 outcome-unknown 时不会向未知
   前台窗口发送通用 Undo。旧 `SelectionCoordinatorBridge`、`ManagedSelectionCoordinator`、
   `TauriSelectionApi` 与重复 session 真相已删除，正式 Selection 热键和 preview commands 调用 Core。
8. Selection Voice 的 intent、prompt、preview owner、confirm/cancel/revert、自动分类 fallback、
   stale-session guard、shutdown 和 typed lifecycle event 已进入 core；新增的 `process_transcript`、
   `prepare_edit` 与 `edit_preview` 高层 use-case 统一 transcript correction、instruction polish、自动
   intent model、输出模式、EditPlan/translation 和 QA preview revision。13 项 contract 覆盖模型
   prompt/输入、翻译 target、direct action、conversation action、首次 preview 与单步 revision；QA
   问答与编辑预览通过稳定 `conversation_id` 关联。
9. `LinuxSelectionRuntime` 已通过 fcitx5 读取并在 commit 前重新校验 selection；变化或取消的 target
   返回 `Cancelled`，无法安全保留 preview/revert 的路径明确返回 `Unsupported`。

**完成状态与剩余原生验证**

1. **已完成（Core/Tauri 业务边界）**：`selection_voice*` prompt snapshot、intent confirm、cancel、
   preview query/ticket/finish/revert commands 已直接调用 Core Selection Voice/QA Interface；原始 ASR
   transcript 直接交给 `process_transcript`，编辑分支只消费 Core `SelectionVoiceEditAction`，QA 编辑
   只调用 `edit_preview`。`SelectionVoiceHostState` 只保存物理热键仲裁、录音资源和 opaque insertion
   target，不复制 selection text、instruction、intent、preview 或业务 phase。Tauri 中的 correction、
   instruction polish、自动意图 LLM、EditPlan、translation、preview answer 和 output-mode 判断均已
   删除；Coordinator 只保留物理热键、QA panel 优先级、窗口创建/聚焦、录音和平台插入。
2. **已完成（兼容契约）**：旧 React command 名、
   camelCase/nullable 字段、事件 payload、窗口来源授权和错误字符串的 compatibility contract。
   `SelectionSnapshot` 与 Selection Voice apply outcome 已有稳定 serde fixture；当前变更保持
   `BACKEND_CONTRACT_VERSION = "1.0.0"`，没有用版本升级掩盖 wire 破坏。
3. **已完成（headless 移交）**：headless example 和 deterministic fixture 演示 preview → confirm、preview → cancel、
   stale session 被拒绝、apply outcome-unknown 不自动重试，以及 Linux preview/revert 稳定返回
   `Unsupported`；示例不创建窗口、不读取真实选区、不实现任何 egui 控件，并已实际运行通过。
4. 在 Windows Tauri 上验证真实选区 capture/preview/revert/窗口切换，在 Ubuntu/fcitx5 上验证
   capture/commit/cancel/Unsupported 分支；原生证明完成前不能只凭 17+13 项 contract 宣布领域收口。

**退出条件**：Tauri Coordinator 不再拥有 selection session/preview 真相；Tauri 与 Linux 通过同一
`SelectionApi` 得到一致状态，宿主仅实现选区读取、目标恢复、窗口和文本插入 seam；Core、Tauri
Adapter、React wire 与 Linux headless contract 在同一最新工作树全部通过。

#### 8.3.5 QA 与 Less Computer 会话

**当前已完成**

1. `QaApi`、`QaRuntimeAdapter`、`QaProgressSink`、`QaInput`、`QaTurnRequest/Result` 和稳定
   `QaSnapshot` 已进入 core Interface；message log、recording/thinking/approval/completed/cancelled/
   failed phase、edit-instruction mode、pending approval token 和公开错误均由 `QaService` 表达。
2. `QaService` 已实现文本/语音 turn、selection 防注入 envelope、recording level、answer delta、
   stale-result guard、cancel/dismiss 幂等、provider 错误脱敏和 shutdown cancel；`session_id` 是每轮
   generation token，`conversation_id` 是成功多轮间稳定的 Selection Voice preview owner；15 项
   core contract 覆盖文本、语音、多轮、取消、迟到回答、approval token、错误脱敏、preview 清理和 shutdown。
3. `EventBus` 已由每个 backend 实例持有 2048 条有界 replay；`EventReplay` 显式返回
   `oldestSequence/latestSequence/truncated`，Less Computer sync 从 core replay 续接，不再依赖
   进程级静态 event log。
4. `TauriQaRuntimeAdapter` 已只持有 recorder/ASR、selection capture 的 opaque host context 和
   LLM/Coding Agent runtime 资源；生产 `BackendDependencies.qa_runtime` 已由 `QaService` 消费。
5. QA hotkey、Esc、overlay finalize、`qa_toggle_recording`、`qa_submit_text`、edit-instruction 和
   dismiss 已接到同一 `QaApi`；Selection Voice 问答与编辑预览也复用同一 Core 链路。
6. 独立 `QaHostState` 已完全删除；Coordinator、`TauriQaRuntimeAdapter` 与 `TauriHostActions` 共享
   一个 `TauriQaHostContext`，其中 `AtomicBool` 只表达 Tauri panel 可见性，业务 phase/messages/
   cancel 仍只属于 `QaService`。dismiss 使用稳定 `conversation_id` 清理匹配的 Selection Voice preview。
7. Tauri QA Adapter 4 项 contract 已通过（含共享 show/clear 可见性）；React 已处理
   `awaiting_approval` 与 `cancelled` 终态；
   QA snapshot resync 与 live event 共用同一字段转换，lagged 后不会产生第二套 phase/可选字段规则。
8. QA panel、Less Computer window、键盘焦点、macOS NSPanel、热键优先级和 shortcut recording
   仍留在 Tauri host；这些不进入 `QaSnapshot`。
9. `begin_recording`/`submit_text` 在 `HostAction::ShowQa` 失败时按 session/phase 原子回滚，
   不启动 recorder/prepare runtime；失败后可立即重试且不会残留 `Recording`/`Thinking` 活跃态。
10. Less Computer 的文字入口已直接调用 `OpenLessBackend::submit_less_computer`；语音入口在
    Tauri 负责录音/native ASR 后，使用 `submit_less_computer_with_session` 把同一 session 交给
    Core。Tauri 不再构造 provider/model/permission/prompt/guard/continuation，也不再发射重复的
    `user/delta/tool/approval/terminal` 事件；这些全部来自 `LessComputerService` 的 typed event。

11. QA 编辑预览的 opaque insertion target 绑定已收窄为构造阶段注入的
    `TauriQaHostContext` callback；QA Adapter 不再从 `AppHandle` 反查 `Coordinator`，也不持有
    Coordinator 强引用。focused QA test 与 `shared-backend-wire-contract` source contract 已覆盖
    该 seam，关闭时由 weak callback 自动失效。

    语音热键入口已补上 Core capture lease：按下先创建 Core session，宿主 recorder/ASR 与
    `submit_less_computer_with_session` 共用该 id；空转写、启动失败和取消会释放未提升的 lease。
    Starting pending stop、静音自动停止仍由 Coordinator 兼容层调用 `end_session`，但只负责宿主
    录音/ASR 资源和热键生命周期，不得向 Linux egui 暴露其 `state`。Core 仍是 Agent provider、
    prompt、guard、approval、continuation、stream、cancel 和终态的唯一来源。

**剩余步骤（必须按顺序完成）**

1. **已完成**：QA/Selection Voice/Remote Input 的 React source compatibility fixture 已覆盖
   command 名与 camelCase 参数、`awaiting_approval`/`cancelled`/`error`、typed QA event 字段、
   Remote status/error listeners 及 lagged resync；完整 `npm.cmd test` 58 项通过。
2. **已完成（实例隔离）**：Less Computer 工具审批复用实例级 `CodingAgentApi.approve`，静态 approval
   registry 已删除；contract 证明 token 不能跨 backend 实例解析。
3. **已完成（compatibility UI）**：Less Computer mount 先建立实时订阅并暂存 pending，再以
   `afterSequence` 读取 `replay_events_after(sequence)`；按 replay 后 pending 的顺序合并，带 seq
   事件按最大水位去重，无 seq fallback 保留。`truncated=true` 时清空旧派生时间线、把水位重置为
   `oldestSequence - 1` 并从本次保留 replay 重建；同步期间新事件、重复 sequence 与截断重建已有
   可观察 TypeScript contract。Linux/egui view model 必须实现同一语义，不复用 React 状态。
4. **已完成**：为 Linux/headless 提供 `QaRuntimeAdapter` fixture 和显式 `Unsupported` 示例；egui 只消费
   `QaSnapshot`/typed events，不依赖 Coordinator、WebView backlog 或 Tauri window label。
5. **已完成（Less Computer Core seam）**：`begin_less_computer_capture`、active session、
   capture cancellation/abort、同 session submit，以及 `LessComputerRunRequest/Result`、
   `LessComputerRuntimeAdapter`、
   `submit/cancel/dismiss/approve`、有界 continuation、approval timeout、stale stream 丢弃、唯一
   `Completed/Failed/Cancelled` 终态及 `Unsupported` 语义已由 Core contract 覆盖；Tauri
   `TauriCodingAgentApi` 只实现进程/Git/临时护栏文件/stream transport。egui 只需要调用 facade、
   订阅 `LessComputerEvent`、按 `seq` 去重并回传 approval。
6. 在 Windows/macOS/Android Tauri 与 Ubuntu Linux host 上取得对应原生运行证明；本地 contract
   不能替代平台 recorder、selection capture、窗口和关闭生命周期验证。

**退出条件**：QA/Less Computer 的会话真相只在 core；Tauri command 只保留窗口来源授权与 wire
转换，Linux 不需要 Coordinator 或 WebView event backlog 即可驱动自己的 view model。

#### 8.3.6 Remote Input

**当前已完成**

1. `RemoteInputApi`/`RemoteInputRuntimeAdapter` 已冻结 status、configure、locale、显式 PIN read/
   rotation、local IP、connect/disconnect、start/feed/stop/cancel stream；status 是无 I/O 的同步快照，
   transport 与 secret persistence 仍为 async。
2. `RemoteInputService` 已拥有 enable/disable/port 状态转换、PIN 生命周期、locale、连接/session
   关联、64 KiB 上限的非空偶数字节 signed Int16LE frame 校验、重复/迟到 PCM guard、端口错误分类、
   typed status/error event 和 shutdown 清理。PIN 不进入 snapshot/event/serde/Debug；8 项 core
   contract 通过。
3. `TauriRemoteInputRuntimeAdapter` 只承载 PIN 文件、TLS/WSS server handle、local IP 和共享 backend
   external dictation 桥接；认证后的连接与所有 PCM lifecycle 调用 Core。Coordinator 的 server、
   refresh generation/lock、PIN、locale、no-insert 状态和旧 persistence tests 已删除。
4. settings diff、启动恢复和 remote commands 已调用 Core；Tauri `cargo check --lib` 与 7 项
   `remote_` 定向测试通过。托盘从 Core status 读取 locale，托盘刷新失败不改变业务结果。

**剩余步骤（必须按顺序完成）**

1. **已完成（定向 contract）**：`get_remote_input_status` 保持旧
   `running/port/pin/urls` shape，PIN 只由这个显式 secret command 注入；core status/event 不含
   PIN。共享 React source contract 同时固定 locale、status/error listener 与 lagged resync 接线。
2. **已完成（本地 Adapter contract）**：PIN 认证失败先于 `connect` 且继续使用 constant-time
   compare；每连接仅一个活动 stream，重复 start 返回 Busy 且保留原 lease；disconnect 必须 cancel，
   stop/cancel 后拒帧，服务 restart 取消旧 session 并让旧 connection/session 返回 `Cancelled`。
3. **已完成（headless fixture）**：`RecordingRemoteInputRuntime` 提供不绑定 socket 的内存 transport，
   记录 server/audio start/stop/cancel 与 PCM frame；未注入生产 transport 时仍走稳定
   `Unsupported`，capability 不得伪造为 available。
4. 在真实宿主验证证书安装、端口占用、WSS/H5、局域网 IP 与长连接 shutdown；Windows 本地
   contract 不能替代 Linux/macOS socket、证书或防火墙证明。
5. **已完成（接口手册）**：fixture、错误码、16 kHz mono signed Int16LE、64 KiB frame 上限、
   单 stream/restart/stale lease、幂等与 secret 规则已写入接口手册；后续破坏性变更才提升
   `BACKEND_CONTRACT_VERSION`。

**退出条件**：core 可以用内存 transport + external PCM fixture 完成远程听写 contract；Tauri
command/remote server 不再拥有业务 session、PIN 或 locale 的第二份真相。

#### 8.3.7 旧 Coordinator 与宿主耦合删除

1. 用 command/event baseline 逐项确认所有业务入口已有 core use-case 和 compatibility test。
2. 删除 `Inner.app`、业务路径上的 `AppHandle`、直接 `emit*`、`tauri::async_runtime::spawn` 和由
   Coordinator 持有的重复领域状态；仅保留真正的 Tauri host orchestration。
3. 把窗口、托盘、updater、dialog、shell、autostart、single-instance 和 Android JNI/overlay/IME
   移到明确 host Module；这些代码不得被 Linux package 引用。
4. 对删除后的 import、State 管理、`manage(...)` 和 handler registration 做残余引用检查；不得保留
   无消费者 manager 来掩盖迁移不完整。
5. 重新生成 command/event baseline；任何数量变化都必须有兼容说明和前端调用点证据。
6. 删除 `core_adapters.rs` 中已永久禁用的 `legacy_cloud_asr`、`legacy_cloud_polish` 和
   `legacy_omni` 迁移考古副本；对应行为只由 Core provider contract 保留。最终源码门禁应拒绝
   Tauri 重新出现第二份 endpoint/model/credential/cancellation 协议构造逻辑。
7. 已删除无生产消费者的 `Coordinator::{start,stop,cancel}_dictation*` 兼容 facade；测试直接
   覆盖宿主 helper，生产 React/CLI/Android/热键入口统一调用 Core。style-pack prompt 诊断和
   ASR vocabulary priority 同样已移入 Core，source contract 防止这些业务规则回流。

#### 8.3.8 2.0 Interface 冻结与 egui 移交

1. **已完成**：只 re-export facade、DTO、errors、events、capabilities、fake/headless fixtures；core 私有
   repositories、transport 和状态机实现不进入 egui 可依赖面。
2. **已完成**：更新 `linux-egui-backend-contract.md`，逐项记录方法、字段、单位、nullable、幂等、取消、超时、
   outcome-unknown、事件顺序、lagged resync、线程规则和 capability 降级。
3. **已完成**：headless example 演示 lifecycle、dictation、settings/history/style-pack、Local ASR、
   Marketplace、Selection、Selection Voice、QA、Remote Input 的可用或 `Unsupported` 分支；Selection
   还必须覆盖 preview/confirm/cancel/stale/outcome-unknown 和 Linux preview/revert `Unsupported`；
   示例不创建 egui 窗口，当前版本已实际运行通过。
4. **已完成**：更新 Linux capability fixtures 和 view-model 映射表；每个未接线能力明确显示 unavailable，不能
   用 fake 成功状态冒充生产支持。
5. **本地已完成，原生 runner 待完成**：以固定审查基线
   `a569a8749188e7843d426f159523193c8d5363ce` 运行第 12 节 Windows 本地门禁并记录命令与测试数。
   当前冻结版本为 `BACKEND_CONTRACT_VERSION = "2.0.0"`；破坏性变更必须附迁移说明。
   Android/macOS/Ubuntu 与发行包证据继续由 12.4 的未勾选项约束。

#### 8.3.9 Provider 验证与模型列表迁入 Core（核心迁移已完成；发布前收口进行中）

这是本轮已收口的 provider 管理面迁移。云端 ASR/LLM/Omni 的正式运行与
`validate_provider_credentials` / `list_provider_models` 现在都由 `openless-core::ProviderService`
承载；Tauri command 仅保留参数/旧 wire/error 转换，Linux 的
`LinuxBackendBuilder::from_shared_providers` 注入同一 service。真实网络、Secret Service 和各平台
原生 runner 仍属于 M9/M10 的独立证据，不能由本地 fixture 代替。

**目标边界**

- Core 拥有 provider 类型解析、channel-scoped credential 读取、默认 endpoint/model、协议选择、
  endpoint 安全校验、验证请求、静态/远端模型列表、超时/取消和稳定错误码。
- Tauri 只负责旧 command 参数解析（`kind` 字符串、可选 `channel_id`）、调用 Core、旧 JSON/错误
  字符串兼容和 React event/wire 转换；不得再持有 HTTP/WS 请求或 provider 分支。
- Linux 生产 factory 注入与 Tauri 完全相同的 Core `ProviderApi` 实现；egui 只调用公开
  `BackendServices::provider`，不读取凭据、不构造 client、不选择协议。
- native/local ASR 的模型加载仍属于 Local ASR Adapter；本节只迁移 provider “连通性验证”和“模型
  列表”管理面，不能把 macOS/Windows 专属 native runtime 引入 Linux workspace。

**建议文件与责任人**

| 文件/目录 | 变更 | 主责 |
| --- | --- | --- |
| `openless-all/app/crates/openless-core/src/provider_service.rs`（新增） | `ProviderService`、credential resolver、validate/list_models 分派与错误映射 | Core 负责人 |
| `openless-all/app/crates/openless-core/src/provider_rules.rs` | 汇总默认值、协议判定、endpoint/model 校验；删除重复规则 | Core 负责人 |
| `openless-all/app/crates/openless-core/src/provider_service.rs`（module tests） | channel credential 隔离、Omni channel 拒绝、静态/远端模型解析、秘密边界和错误映射 contract | 测试负责人 |
| `openless-all/app/src-tauri/src/commands/providers.rs` | 仅保留 command 参数和旧 wire/error 转换，删除业务实现 | Tauri 负责人 |
| `openless-all/app/src-tauri/src/core_adapters.rs` | 注入共享 `Arc<ProviderService>`，删除 `TauriProviderApi` 反向代理 | Tauri 负责人 |
| `openless-all/app/linux-egui/src/backend.rs` | `from_shared_providers` 注入同一 Core service | Linux host 负责人 |
| `openless-all/app/linux-egui/src/backend.rs`（module tests） | 断言 Linux factory 的 provider 非 `Unsupported` 且不依赖 Tauri | Linux host/测试负责人 |
| `openless-all/app/scripts/shared-backend-wire-contract.test.mjs` | command 源码门禁：禁止 vault/HTTP/provider 构造回流 | 测试负责人 |
| `docs/linux-egui-backend-contract.md` | provider 请求、错误、能力和版本契约 | 架构负责人 |

**实施步骤记录（核心迁移与宿主接线已完成；发布前收口项见下）**

1. **冻结输入输出契约。**
   - 保留公开 `ProviderRequest { kind, channel_id }`、`ProviderCheckResult` 和
     `ProviderModelsResult` 的 serde 字段；`channel_id = None` 继续表示当前 active channel，不能
     静默改变旧 React 行为。
   - 为 `ProviderApi` 增加文档化的超时、取消、幂等和错误映射：参数/模型缺失用
     `InvalidArgument`，凭据缺失用 `Provider`（带可操作的稳定 sentinel），网络/HTTP/WS 失败用
     `Provider`，取消用 `Cancelled`，未接线能力用 `Unsupported`；不得把错误统一压成普通字符串。
   - 明确秘密边界：API key、token、Authorization、device code、完整 endpoint credential 不得出现在
     DTO、`BackendError.details`、日志、`Debug` 或测试 fixture；验证结果只返回 `ok` 或脱敏错误码。
   - 在 `docs/linux-egui-backend-contract.md` 增加 provider 表格：请求字段、默认值、验证是否发真实
     请求、模型列表是静态还是远端、超时上限、可重试性和 Linux capability。

2. **建立 Core-owned credential resolver。**
   - 在 core 新增窄的 `ProviderCredentialResolver`（或等价私有 module），只依赖
     `CredentialStore::read(CredentialKey)`；按 `CredentialNamespace::{Asr,Llm,Omni}` 和 channel id
     读取 key/endpoint/model/extra headers/temperature/advanced config。
   - 将 `ProviderScope` 的 channel 规则迁入 core：ASR/LLM 允许 channel id，Omni 明确拒绝 channel id；
     channel 的 `provider_type` 来自非秘密 metadata，不能用 id 猜协议；`None` 回退 active provider
     只能由 resolver 统一完成。
   - 将 `CredentialAccount` 到 core `CredentialKey` 的映射集中定义并加单元测试，验证 channel A/B
     不串 credential，active 切换不改变已捕获的请求配置。
   - resolver 返回不含秘密的 `ResolvedProviderSummary`（provider type、model、endpoint 是否配置、
     auth mode），真实 secret 仅在构造请求的短生命周期对象中存在。

3. **迁移 provider 规则和默认值。**
   - 将 `parse_provider_kind`、默认 endpoint/model、Bailian endpoint 派生、StepFun 模型协议判定、
     DashScope/Whisper 请求格式、模型白名单和 URL scheme 校验迁入 core `provider_rules`/provider
     service；已有同名规则只保留一份实现。
   - 复用 core 已有的 `SharedCloudTranscriptionEngine`、`SharedCloudTextPolisher`、
     `SharedOmniDictationEngine` 构造路径，确保验证请求和正式运行请求使用相同 provider type、
     channel、model、endpoint 与 credential account。
   - provider-specific 常量（Bailian、Qwen realtime、Volcengine、Xfyun、StepFun、Mimo、
     ElevenLabs、DashScope、Codex OAuth 等）放在 core provider module；只有 native engine 和平台
     文件路径留在 Adapter。

   验证策略必须逐 provider 固定，不能由 Tauri 继续隐式决定：

   | provider 类别 | Core 验证动作 | 模型列表 | 关键凭据/规则 |
   | --- | --- | --- | --- |
   | OpenAI-compatible LLM/ASR | 最小 chat completion 或真实 transcription 请求 | 远端 `/models`（失败即明确错误） | endpoint URL、model 必填；LAN 无鉴权模式按 provider 规则允许空 key |
   | Gemini LLM/Omni | `generateContent`/等价最小文本探活 | Core 静态或 Gemini 列表转换 | API key、endpoint、model 按 channel 读取 |
   | Codex OAuth | 使用已保存 OAuth 状态执行最小 polish/授权检查 | Core 静态 Codex 模型清单 | token 只在 secret store；失效返回脱敏 OAuth 错误 |
   | Bailian classic/Qwen realtime | WSS 握手 + 最小静音帧 + 收尾 | Core 静态清单，按模型分协议 | endpoint 按协议派生；必须校验 `ws/wss` scheme |
   | Volcengine/Xfyun/StepFun realtime | 对应 WS 鉴权、session.update、静音收尾 | Core 静态清单 | 多字段鉴权、模型协议判定和错误码归 Core |
   | Mimo/ElevenLabs/DashScope batch | 规范 WAV/官方示例音频的真实 HTTP 请求 | Core 静态清单或受限远端列表 | 响应大小、超时、示例音频和模型白名单固定 |
   | local/native ASR | 不在本节验证；转交 `LocalAsrApi` runtime | 由 Local ASR catalog 提供 | 无法接线时返回 `Unsupported`，不把 Tauri native runtime 带入 Linux |

4. **实现 Core `ProviderService`。**
   - 新增 `ProviderService { credentials, task_spawner, http_client_factory/transport }`，实现
     `ProviderApi::validate` 与 `ProviderApi::list_models`；构造时注入 `CredentialStore`，不访问
     Tauri `State`、`AppHandle` 或全局 vault。
   - `validate` 按 `ProviderKind` 和 resolved provider type 分派：LLM/Codex OAuth、传统 HTTP ASR、
     realtime WS ASR、Omni 文本探活分别调用对应 core provider；静音音频、示例音频和握手收尾规则
     与正式 provider 实现保持一致。
   - `list_models` 对无远端列表接口的 provider 返回 core 静态清单，并先执行与 validate 相同的凭据/
     endpoint 校验；对 OpenAI-compatible 等远端列表接口使用 core HTTP transport，限制响应大小、
     禁止 redirect 到未经允许的地址并做 JSON schema 校验。
   - 所有请求使用显式 timeout 和 cancellation token；超时不得隐式切换渠道或重复发起可能产生
     计费的探活请求。请求完成后释放 secret 和 transport handle。
   - 将当前 Tauri sentinel（例如 `providerHttpStatus:*`、`endpointInvalid`、`asrModelMissing`）
     转成 `BackendErrorCode` + 稳定 machine detail；Tauri 兼容层再把 code 映射回旧字符串，Core
     本身不依赖中文文案。

5. **补齐 provider fake/transport fixture。**
   - 提供 `FakeProviderTransport`，可按 endpoint/model 返回成功、401/403/429/5xx、超时、无效 JSON、
     redirect 和取消；fixture 不保存真实 key。
   - 提供 `InMemoryCredentialStore` 的 channel A/B、active provider、缺失凭据和错误注入场景；每个
     测试使用唯一临时目录并自动清理。
   - 为静态模型清单、远端模型清单、空模型、未知 provider type、Omni channel 拒绝、Bailian/StepFun
     双协议和 Codex OAuth fallback 建立 serde/行为 fixture。

6. **迁移 Tauri Adapter。**
   - `TauriProviderApi` 改为在构造时持有 `Arc<dyn ProviderApi>`（或直接复用 Core service），只做
     `String -> ProviderKind`、`ProviderRequest` 构造和 `BackendError -> legacy String/JSON` 转换。
   - 删除 `commands/providers.rs` 中的 `ProviderScope`、credential 读取、`ProviderConfig`、HTTP/WS
     validation、model-list 分支、provider-specific request body 和 provider 错误分类；文件只保留
     `#[tauri::command]` 薄函数及兼容转换。
   - 删除 `TauriProviderApi` 对 `validate_provider_service`/`list_provider_models_service` 的反向
     调用；source contract 必须拒绝 `CredentialsVault::get*`、`reqwest::Client`、provider 构造器和
     `tokio::time::timeout` 在该 command module 重新出现。
   - 保留旧 command 名、参数 key、nullable 语义和 React 错误映射；新增字段只允许向后兼容，破坏性
     变更必须提升 `BACKEND_CONTRACT_VERSION` 并附迁移说明。

7. **接入 Linux 生产 factory。**
   - `LinuxBackendBuilder::from_shared_providers(config)` 打开一次 `LinuxCredentialStore`，构造
     Core `ProviderService` 并写入 `services.provider`；不得让 egui 注入 provider factory 或 credential
     account。
   - `LinuxBackendBuilder::new(...)` 的显式 provider 注入仅用于测试/特殊宿主；测试构造仍可用 fake
     `ProviderApi`，但生产入口必须经过 shared factory。
   - Linux 无法提供 native/local runtime 时只对对应 Local ASR 能力返回 `Unsupported`；云端 provider
     validate/list 不得因为 UI 非 Tauri 而返回 `Unsupported`。
   - capability snapshot 增加 provider 管理面状态（configured / unconfigured / unsupported），但
     不泄露 key；egui 根据 snapshot 和错误码显示降级文案。

8. **补跨宿主 contract 与回归测试。**
   - Core contract：channel scoped credential、provider/type/model 冻结、每种协议的成功/失败/取消、
     静态/远端模型列表、秘密不泄漏、超时不重复、未知 provider 和 Unsupported。
   - Tauri wire contract：旧 command 名、camelCase 字段、错误 sentinel、Codex/Omni/ASR 分支结果与
     现有 React fixture 一致；command 源码 contract 证明不含业务实现。
   - Linux host contract：使用同一 fake credential/transport 调用 `services.provider.validate` 和
     `list_models`，证明不经过 Tauri；生产 factory smoke 至少断言 `services.provider` 不是
     `UnsupportedDomainServices`。
   - 将 provider contract 纳入 M9 门禁和 `shared-backend-wire-contract.test.mjs`；任何 Tauri provider
     业务 token 回流、Linux factory 未注入或 channel credential 串线都必须使门禁失败。

9. **删除旧实现并做残余审计。**
   - `rg` 检查 `commands/providers.rs` 不再出现 `CredentialsVault`、provider HTTP/WS client、
     provider struct constructor、模型列表静态清单和协议分支；残余只允许兼容转换函数。
   - `rg` 检查 core provider module 不出现 `tauri::`、`AppHandle`、window label、React event name；
     Linux crate 不出现 Tauri/WebKitGTK。
   - 重新生成 command/event baseline，运行 `cargo fmt --check`、Core/Linux clippy、Core/Tauri/Linux
     provider tests、frontend contract、完整 Tauri suite 和 `git diff --check`。
   - 记录迁移前后 provider 请求/错误行为差异；若发现行为变化，先补 fixture 再改实现，不通过修改
     React 调用方掩盖兼容问题。

**当前实现的发布前收口项（不阻塞 egui UI 开发，但阻塞 provider 生产发布）**

- **已完成：可替换 transport 与 fake 覆盖。** `ProviderService` 通过 `ProviderTransport` 注入模型列表
  请求；生产使用无 redirect、显式 15 秒 timeout、2 MiB response 上限的 reqwest 实现，测试使用
  `FakeProviderTransport` 覆盖 401/403/429/5xx、timeout、connection/request、cancel、invalid JSON、
  response-too-large 和 redirect 状态，并验证 URL/header value 不进入 `Debug`、错误或 fixture 输出。
- **已完成：静态模型 parity。** Core 静态清单已按迁移前 Tauri 顺序补齐 Bailian/Qwen realtime、Mimo、
  Fun-ASR、ElevenLabs 与 Codex OAuth 条目；测试固定每个 provider 的顺序和去重，后续新增模型必须先更新
  parity fixture，不能把“可返回列表”误认为完整 parity。
- **已完成：显式 channel-scoped LLM 写入。** `temperature` 与 `extra headers` 写入按指定
  `provider_id` 定位，不再无条件写 active channel；A/B channel 回归测试证明 active channel 不会被旁路
  修改。读取期间的 provider snapshot 仍必须保持同一 channel 语义。
- **仍未完成：真实 provider 与平台证据。** 真实 provider 网络、Secret Service/keyring、取消/超时在真实
  runtime 下的行为，以及 Android/macOS/Windows/Ubuntu runner 证据仍按 M9/M10 执行；本地 parser、unit、
  fake transport 和 WSL contract 只能证明纯函数与接线，不能代表真实服务可用。

**本节退出条件**

- `ProviderApi` 的验证和模型列表在 Core 只有一份可测试实现，Tauri 与 Linux 使用同一实例语义。
- Tauri provider command 只剩参数转换、Core 调用和旧 wire/error 兼容；源码门禁无业务副本。
- Linux 生产 factory 不再把 provider 管理面设为 `Unsupported`，egui 可在无 Tauri 环境调用公开接口。
- Core/Tauri/Linux provider contract、秘密扫描、依赖方向、完整本地门禁全部通过；真实网络、keyring
  和平台 runner 仍按 M9/M10 单独留证，不能用 fake 证明正式发布。

### M0：冻结决策和兼容契约

**任务**

1. 确认目标平台：macOS/Windows = Tauri，Linux = egui，Android = 继续 Tauri mobile。
2. 确认 Linux egui 与 core 是否同进程；本计划默认同进程直接 Rust 调用。
3. 建立当前 command 名称、参数、返回 JSON、事件名和能力字段的基线清单。
4. 用 `rg` 统计所有 `tauri::`、`AppHandle`、`emit`、`listen` 和 `#[tauri::command]` 使用点，保存为迁移 checklist。
5. 标记 UI 专属命令：窗口打开/关闭、focus、拖动、动画和 WebView bridge 不进入 core。
6. 写下 Android 保持 Tauri 的决定，避免迁移时误删 mobile 分支。

基线漂移检查由 [`scripts/check-command-event-baseline.ps1`](../openless-all/app/scripts/check-command-event-baseline.ps1)
执行；它会从当前 `lib.rs` handler 宏重新提取 command 名称，并拒绝缺失、意外新增、重复或计数不一致。

**产物**

- 本计划文档完成评审。
- `docs/linux-egui-backend-contract.md`（M7 生成的接口手册）目录和版本策略确定。
- [`docs/linux-egui-command-event-baseline.json`](./linux-egui-command-event-baseline.json)：
  机器可读的 command/event/capability 基线清单（当前观察到 196 个 Tauri command、30 个
  legacy event、29 个 core event kind）。
- 每个模块的 owner、依赖和迁移顺序表。

**验收**

- 任何新增公共接口都能回答“core、Tauri host 还是 Linux host 的职责”。
- 产品与发布决策显式列在本文档“M0 决策记录”，不通过临时代码默认。

### M1：建立 package 骨架和依赖门禁

**任务**

1. 创建 `crates/openless-core`，加入最小 `Cargo.toml` 和空 facade。
2. 创建 `linux-egui` package 的空 host stub；不实现 UI，只验证能依赖 core。
3. 将 `src-tauri` 标记为 Tauri 适配器（现有 Cargo package 名称可继续为 `openless`），先不改变 React command 名称。
4. 根 workspace 只包含 core/Linux 并使用根 `Cargo.lock`；Tauri 与 backend compatibility tests 使用独立 manifest/lockfile，避免 Linux 解析 macOS/Tauri dependency。
5. 为 core 增加依赖检查脚本：core 的正常依赖树不得包含 `tauri`、`wry`、`webkit2gtk`、`egui`、`eframe`。
6. 为 Linux package 增加同样的依赖检查；允许 Linux 原生窗口依赖，但不得出现 Tauri/WebKitGTK。
7. 审计 `qwen3-asr-rs`、`qwen-asr` 等 vendored path；Tauri 作为独立 manifest 保留 native runtime，根 core/Linux workspace 显式 exclude Tauri，避免 macOS-only path 在 Linux 元数据阶段被解析。
8. 保留现有 Tauri package 能独立 `cargo check` 的能力。

依赖门禁脚本为 [`scripts/check-core-deps.ps1`](../openless-all/app/scripts/check-core-deps.ps1)，
接受 `openless-core`（默认）或 `openless-linux-egui` 作为 package 参数。

**验收**

```text
cargo check --locked -p openless-core
cargo check --locked -p openless-linux-egui
cargo check --locked --manifest-path "src-tauri/Cargo.toml" --lib
pwsh -NoProfile -File "scripts/check-core-deps.ps1" openless-core
pwsh -NoProfile -File "scripts/check-core-deps.ps1" openless-linux-egui
```

### M2：迁移共享类型、错误和能力模型

**任务**

1. 从 `types.rs` 提取平台无关的枚举、DTO、快照和 serde 字段。
2. 保持现有 `camelCase` / `snake_case` / `lowercase` 序列化名称，避免 React IPC 破坏。
3. 把 `CapsulePayload` 重命名或包成 core 的 `DictationStateSnapshot`；Tauri adapter 提供旧名字映射。
4. 建立 `BackendError` 和错误码，command 层暂时保留旧字符串输出的兼容转换。
5. 将 `PlatformCapabilities` 的结构放入 core；`current()` 改为由 host 传入或由平台 adapter 构造。
6. 将秘密字段、日志字段和 UI 可见 status 分开，禁止快照包含明文凭据。
7. 把纯类型测试移到 core，确保 JSON fixture 与现有前端契约一致。

**验收**

- `openless-core` 不包含任何窗口、WebView、Tauri 或 egui import。
- React 现有 command 的字段名和枚举值不变，或有显式版本兼容转换。
- core 类型测试、serde fixture、错误码测试全部通过。

### M3：定义宿主 Interface、依赖注入和生命周期

**任务**

1. 定义 `BackendConfig`、`BackendDependencies`、`TaskSpawner`、`Clock`、`HostActions`。
2. 把凭据、录音、文本插入、热键、资源目录等真实变化点定义为最小 Interface。
3. 提供 `InMemoryCredentialStore`、`FakeRecorder`、`RecordingInserter`、`FakeHotkeyController` 和 `HeadlessHostActions`。
4. 明确每个 Interface 的线程安全、超时、取消和错误语义。
5. 在构造阶段完成依赖注入；禁止 core 在方法内部自行 `new` Tauri plugin 或系统窗口。
6. 增加 `start()` / `shutdown()` lifecycle tests，包括重复调用、启动失败和取消中关闭。

**验收**

- 无真实麦克风、窗口或 keyring 时可以构造 core 并运行状态机测试。
- fake inserter 能验证成功、fallback、失败和 outcome-unknown，不需要 Tauri。
- shutdown 后没有后台 task、录音或热键泄漏。

### M4：抽取 Coordinator 和统一事件总线

**任务**

1. 将 `Coordinator` 拆为 core coordinator 与宿主 host action/event bridge。
2. 移除 `Inner.app: Mutex<Option<AppHandle>>`，替换为 core event bus 和 `HostActions`。
3. 把 `tauri::async_runtime::spawn` 替换为 core executor Interface。
4. 将 `emit_to("capsule", ...)`、`emit_to("qa", ...)`、窗口显示/隐藏逻辑移到 Tauri adapter。
5. 把 `coordinator_state` 的 generation/session guard 与所有异步完成路径接到统一 session token。
6. 建立事件顺序、sequence、lagged subscription、最终态唯一发布的测试。
7. 为 `DictationStateChanged`、transcript delta、polish delta、insert fallback 建立最小端到端 fixture。

**验收**

- core 可以在 headless host 中跑完整“开始 → 录音 → ASR → 润色 → 插入 → 终态”测试。
- Tauri 和 Linux host 都能收到同一套语义事件。
- 旧 Tauri 事件名只存在于 adapter，不存在于 core。

**当前收口状态**

- `Coordinator` 已不持有或接收 `AppHandle`/`WebviewWindow`，不直接发射 Tauri 事件；
  `TauriCoordinatorHost::app()` 保持私有。
- capsule 原生窗口行为和窗口状态已归 Host；Coordinator 只计算或传递语义状态与窄值。
- M4 仍保持“进行中”：compatibility Coordinator 尚未完全删除，剩余生产调用必须继续按
  core 业务、Host 原生能力、无生产消费者三类处置，且原生 runner 证明尚未齐全。

### M5：迁移持久化、provider 和业务领域

按低风险到高风险顺序迁移，保持每个阶段可编译：

1. `settings` / `preferences` / `PlatformCapabilities`
2. `history` / `activity` / `dictionary` / `correction`
3. `style_packs` / zip import-export / runtime diagnostics
4. `credentials` / provider channels / OAuth status（保留秘密隔离）
5. provider validation/model-list 管理面（按 8.3.9 迁入 Core `ProviderService`，再进入 ASR/LLM/Omni
   正式请求路径）
6. `asr` / `polish` / `omni` / `net`
7. local ASR model catalog、download、prepare、release
8. remote input server、marketplace、coding agent
9. selection polish、selection voice、QA、Less Computer

每个领域的步骤：

1. 把实现文件移入 core module。
2. 把 `AppHandle`、`State`、window label 和 Tauri plugin 调用替换成宿主 Interface/语义事件。
3. 将原 command 改为薄 wrapper，保留 command 名称。
4. 将原模块测试移入 core interface tests。
5. 用 fake adapter 补齐成功、失败、取消、过期结果和权限降级场景。
6. 更新 Tauri adapter 和 Linux contract 示例。
7. 运行领域测试、core 全量测试和 Tauri compile gate。

`asr` / `polish` / `omni` 的迁移还必须额外满足：provider router 的重复 session 不能覆盖
原路由；所有生产 Adapter 必须按 `DictationContext` 固定到会话；旧 prompt compose 的 XML、
净化、防注入、前台应用、光标上下文、历史 turns、翻译和 user prompt envelope 必须有兼容
fixture。只有“能请求某个 OpenAI-compatible endpoint”不等于完成了旧主链语义迁移。

**当前实施状态**

- settings/preferences 已由 `OpenLessBackend::update_settings` 统一 strict/reconcile、legacy 同步、
  preserve-style、乐观 revision、单写入 gate、显式 effect plan、receipt 补偿及一次持久化/事件。
- history/activity/vocabulary/correction/style-pack/credentials、prompt compose、云 ASR/LLM/Omni、
  Coding Agent、Local ASR、Marketplace、Selection/Selection Voice、QA 和 Remote Input 已有 Core
  Implementation/Interface；操作系统录音、插入、socket、native/local ASR 与授权副作用继续以
  注入 Adapter 表达。
- 旧 `set_preferences*` 公共兼容面与 `legacy-preferences-write` feature 已删除；测试私有 helper 不能
  被宿主启用。Android/macOS/Windows/Ubuntu 原生 effect 与网络/音频/窗口行为仍须由对应 runner
  证明，不能由 Windows contract test 推断。

**验收**

- 同一业务规则只在 core Implementation 中存在；Tauri/Linux Adapter 不保留第二份判断。
- 领域 Interface 的成功、失败、取消、过期结果、权限降级和秘密隔离都有 contract test。
- 对应 Tauri command 仍保持既有名称和序列化语义；Linux 未接线能力明确返回 `Unsupported`。
- 每迁移一个领域即可独立合并和回退，不要求一次性搬完全部领域。

### M6：完成 Tauri 薄适配器

**任务**

1. Tauri setup 只负责构造 dependencies、创建 `Arc<OpenLessBackend>` 和管理 host state。
2. 每个 `#[tauri::command]` 只做参数转换、调用对应 core use-case、错误/DTO 序列化。
3. 建立 `tauri_events.rs`：订阅 core event，映射为当前 React 监听的事件名。
4. 把窗口创建、窗口定位、拖动、透明/点击穿透、vibrancy/Mica、托盘和 menu 放入 Tauri host。
5. 把 updater、dialog、shell、autostart、single-instance plugin 保留在 Tauri host。
6. 把 `restart_app`、system settings、external URL 等系统动作接到 `HostActions`。
7. 保留 Android Tauri host 的 JNI/overlay/IME 分支，避免 core 被 mobile 专属类型污染。
8. 为旧 IPC contract 增加 TypeScript/Rust 交叉测试：command 名称、参数 key、事件 payload 和错误码一致。

**当前实施状态**

- setup 已构造共享 `Arc<OpenLessBackend>`；React command、CLI、Android JNI、remote PCM、桌面普通
  听写热键及已迁移复杂领域通过 Core Interface 调用。
- `backend_dependencies()` 使用同一个 `SystemCredentialStore` 构造 Core 的共享云 ASR、LLM、Omni、
  Auxiliary 与 QA provider；Tauri Adapter 只追加平台录音、native/local ASR、窗口、插入和 runtime。
- Provider 管理面也已共享：`ProviderService` 的 `validate`/`list_models` 读取 Core credential port
  并执行统一 provider 构造/模型解析；`commands/providers.rs` 不再读取 `CredentialsVault` 或发起
  provider-specific HTTP/WS 请求，Linux factory 注入同一 Core service。
- settings/QA/全部快捷键入口已切 Core transaction；`TauriSettingsRuntime` 只消费显式 target，
  Coordinator 的 listener runtime target 与偏好文档分离，style-pack 删除消费 Core outcome。
- command 内旧 settings/hotkey 事务副本及 previous/write/refresh/rollback helper 已删除；
  `core_adapters.rs` 不再通过 `AppHandle` 反取 Coordinator，Local ASR 与 hotkey/QA 只接收构造层
  注入的窄依赖；仍需继续审计 compatibility Coordinator 的非 settings 宿主职责并取得原生 runner
  证据。

**验收**

- React 主窗口、capsule、QA、Less Computer、选择润色和设置页面仍可调用原 IPC。
- `src-tauri` 是唯一出现 `#[tauri::command]` 和 Tauri window label 的 package。
- Tauri adapter 不包含 provider validation/model-list 核心业务分支；source contract 已证明 command
  module 不含 provider 协议、凭据读取和请求构造。

### M7：交付 Linux egui 接口包（本计划负责）

这一阶段不实现 egui UI，只交付让另一组可以开始 UI 开发的完整材料。

**任务**

1. 发布 [`openless-core` 接口手册](./linux-egui-backend-contract.md)，包含：
   - 构造与生命周期
   - 所有领域接口和参数
   - `BackendSnapshot` 字段
   - `BackendEvent` 分类、顺序和 session 规则
   - `BackendErrorCode`
   - `PlatformCapabilities` 能力矩阵
   - 线程、取消、超时和重连规则
2. 提供 `linux-egui/examples/headless_host.rs`，展示构造 backend、订阅事件、执行听写和 shutdown；示例不绘制 UI。
3. 提供 `FakeBackend` 或 fake provider fixture，允许 egui 团队在无网络、无麦克风环境调试页面。
4. 提供事件到 UI view model 的推荐映射表：

   | core 事件 | egui 团队应更新的状态 |
   | --- | --- |
   | `DictationStateChanged` | 录音/转写/润色/完成状态和 level |
   | `TranscriptDelta` | 原文增量文本 |
   | `PolishDelta` | 输出增量文本 |
   | `InsertFallback` | fallback 提示卡片状态 |
   | `PreferencesChanged` | 设置缓存 |
   | `CredentialsChanged` | provider 是否配置，不显示秘密 |
   | `HistoryChanged` | 历史列表失效并重新读取 |
   | `DownloadProgress` | 模型下载进度 |
   | `PermissionChanged` | 权限状态和降级文案 |
   | `HotkeyStatusChanged` | 热键能力/错误状态 |
   | `Notification` | 非阻塞通知队列 |

5. 提供 Linux capability fixture，覆盖 X11、Wayland、fcitx5 可用/不可用、无托盘、无权限和不支持更新器等状态。
6. 提供 headless host 的 contract tests：调用顺序、事件顺序、错误码、取消和 snapshot resync。
7. 给 egui 团队一份“不得依赖内部实现”的检查表，明确只能依赖 core facade、DTO 和 event subscription。
8. 约定接口版本：破坏性字段变更必须更新 contract version 和迁移说明；新增可选字段默认兼容。
9. 设置公共入口只暴露 `LinuxHost::save_settings(preferences, expected_revision)` 和
   `update_settings_strict(preferences, expected_revision)`；调用方先从 `snapshot()` 获取 revision。
   reconcile 入口用于整表兼容保存，strict 入口用于单项/严格保存；两者都由 Core 事务执行 effect、
   持久化和补偿，UI 不得调用低层 `set_preferences*`。
10. 生产构造入口固定为 `LinuxBackendBuilder::from_shared_providers(config)`；它打开 Linux
    `CredentialStore`，注册 Core 共享 ASR/LLM/Omni/Auxiliary、`ProviderService` 和传统 Pipeline。
    Provider 管理面已由 Core `ProviderService` 接线；egui UI 不注入 `TranscriptionEngine`、
    `TextPolisher`、credential account 或 provider router；显式
    provider 注入的 `LinuxBackendBuilder::new(...)` 只用于测试和特殊宿主。
11. Marketplace 由同一生产 factory 注入 `MarketplaceConfig::production()`；UI 通过
    `LinuxHost::download_marketplace_archive(pack_id, target)` 保存 Core 已校验归档。`target` 必须是
    已有父目录下的绝对路径；宿主不得覆盖已有文件，写入失败必须清理不完整文件。

`EventSubscription::try_recv()` 是 egui 帧内消费事件的非阻塞入口；收到 `Empty` 结束本帧
drain，收到 `Lagged` 必须用 `snapshot()` 或领域查询重同步。Linux host contract test
位于 `linux-egui/tests/host_contract.rs`，不创建窗口也不依赖 Tauri。

**交付边界**

- 我们负责 Rust core、host Interface、示例和 contract tests。
- egui 团队负责 `eframe::App`、布局、控件、交互、绘制、输入法体验、视觉和 UI 测试。
- egui 团队不需要修改 core 内部模块；发现缺少能力时提交接口需求和可复现 contract test。

**验收**

- egui 团队只依赖公开 facade、DTO、事件订阅、fake/headless Adapter 和 contract 文档即可开始开发。
- Interface 手册明确字段、线程、顺序、取消、错误、能力降级和版本兼容规则，不要求阅读 core Implementation。
- 未完成的真实 Adapter 返回稳定的 `Unsupported`；示例和 fixture 不把未接线能力伪装为可用。
- provider 验证/模型列表必须来自 Core `ProviderService`；Linux factory contract 已断言该 service
  已接线且未回退为 `Unsupported`，egui 不得复制 Tauri provider 逻辑。
- headless 示例实际执行并覆盖听写、Selection、Selection Voice、stale session 与 outcome-unknown；
  Linux host contract 从公开 API 验证同一能力边界。
- 本阶段最初只移交接口；后续 F01/F02 已把可操作的 egui UI 纳入本 PR。生产 UI 的连续会话、审批和增量显示现纳入复核，视觉设计仍由 egui 团队负责。

### M8：Linux host 接口接线准备

此处保留早期非 UI 适配器阶段的设计；最终生产 UI 与其启动、事件和目标验证见本轮复核记录：

1. 实现 Linux `TaskSpawner`、`HostActions`、`ResourceResolver`、`CredentialStore`。
2. 将 fcitx5 DBus commit、热键同步、选区读取接入 Linux platform adapter。
3. 把 `ensure_plugin_installed(app: &tauri::AppHandle)` 改为资源目录/目标目录接口；Tauri 和 Linux host 各自提供路径。
4. 实现 Linux 单实例、启动器参数、退出和后台生命周期；CLI intent 只转为 core action。
5. 实现 Linux 音频设备枚举、level monitor、录音和插入 fallback adapter。
6. 明确 X11 / Wayland 的支持矩阵和降级行为；不把“egui 能启动”当作 overlay、global hotkey 或 fcitx 全部可用。
7. 输出给 egui 团队的 host capability snapshot 和错误文案。
8. 用统一 `LinuxNativeRuntime` 持有 primary single-instance broker 和 fcitx5 hotkey listener；
   `pump()` 非阻塞 drain intent/event/error，`shutdown()` 先停止并 join 宿主线程，再关闭 core。
9. 在 Ubuntu/fcitx5 记录 translation modifier 与 dictation press 的真实信号顺序，并用时间线
   contract test 固定关联规则；晚到 modifier 不得修改已经启动的 `DictationContext`。
10. 实现 `LinuxSettingsRuntime`，消费 Core 显式 hotkey/active-provider target，通过 fcitx5 DBus 与
    credential metadata 执行平台 effect，并以 typed receipt 逆序恢复。Coding Agent 启用与语音热键必须有真实 fcitx5 effect；其余未实现的 switch-style、open-app、style-pack hotkey 和 Windows keyboard effect 才保持明确 `Unsupported`。
11. 以 `LinuxBackendBuilder::from_shared_providers(config)` 作为唯一生产 factory：UI 只传配置，
    factory 内部创建 Linux credentials、Core `ProviderService`、共享云 ASR/LLM/Omni/Auxiliary
    router、cpal recorder、fcitx5 inserter 和 settings runtime；测试/特殊宿主才使用显式 provider
    注入构造器。factory contract 必须断言 `services.provider` 已接线，不能静默回到
    `UnsupportedDomainServices`。

**验收**

- headless Linux host 可以调用 core 的听写主链路。
- Linux 生产 factory 可以调用共享 Core `ProviderApi::validate/list_models`，不经过 Tauri；provider
  凭据按 channel 隔离且错误/取消语义与 Tauri 一致。
- fcitx5 缺失时返回 `Unsupported`/`Platform`，不会让 core panic 或假装插入成功。
- 资源路径、插件文件和用户目录写入行为在 AppImage/deb/rpm 场景分别有测试。

**当前原生证据与边界（WSL Ubuntu）**

- `secret_service_contract` 已在真实 `dbus-run-session`/gnome-keyring 下显式通过，证明
  `LinuxCredentialStore` 的 set/read/remove 和 secret 不落 metadata；普通 `cargo test` 不运行该
  contract，避免把桌面服务设为默认依赖。
- `fcitx5_contract` 已在真实 fcitx5 加载仓库 plugin 后显式通过，证明 DBus object/method、listener
  启停及 press/release/combined/translation signal 映射；插件在无焦点输入上下文时只记录警告并
  返回，fcitx5 不崩溃。合成 signal 不等价于真实物理按键顺序，仍需桌面 runner。
- `cpal_contract` 已显式通过；当前 WSL 无 ALSA 输入设备，adapter 返回明确的平台/权限/不支持错误。
  真实设备下的 stream start/stop、settings effect/单实例退出和焦点插入仍需 runner。

### M9：测试迁移和质量门禁

**测试层次**

1. **Core unit tests**：状态转换、提示词、纠正规则、数据迁移、provider 规则/默认值/协议判定、错误分类、sequence 和 generation guard。
2. **Core integration tests**：fake recorder、fake ASR/LLM、fake inserter、fake vault、fake provider transport、fake clock 的完整听写链路及 provider validate/list_models。
3. **Adapter contract tests**：Tauri event mapping、Tauri provider wire mapping、Linux host actions、Linux provider factory、能力矩阵和资源目录。
4. **Tauri compatibility tests**：现有 React command/event JSON 不变。
5. **Linux dependency tests**：Linux package 编译不拉 Tauri/WebKitGTK。
6. **Egui UI tests**：由 egui 团队负责；我们只提供 headless backend fixtures，不验收视觉布局。

**现有测试迁移**

- `src-tauri/backend-tests/tests/backend_rust.rs` 及其 Tauri stub 已删除；
  `backend-tests/tests/core_contract.rs` 直接依赖公开 `openless-core`，只验证 framework-independent
  contract。
- 原先被 path include 的纯规则测试归 `openless-core`；Windows IME、macOS host、Linux fcitx
  等平台测试归各自 crate；Tauri 内部单测由 Tauri crate 自身的 `--lib` 测试运行。
- 现有 `src-tauri/src/lib.rs`、`coordinator.rs`、`types.rs` 的纯 Rust tests 随对应
  Implementation 迁移到 core；尚未迁移的测试留在 Tauri crate，不复制第二份源码。
- 保留 Windows IME、macOS host、Linux fcitx 的平台 tests，但不让它们成为 core 的编译依赖。
- 所有会写 repository 的测试必须使用每测试唯一且自动清理的临时 `data_dir`；禁止使用 crate-local
  `"data"`。门禁在测试前后检查 `crates/openless-core/data/` 不存在，避免并行污染和未跟踪产物。

**建议门禁**

以下命令从 `openless-all/app` 执行；本地和 CI 都必须使用已提交 lockfile：

```text
npm.cmd test
npm.cmd run build
cargo fmt --check --all
cargo clippy --locked -p openless-core --all-targets -- -D warnings
cargo test --locked -p openless-core
cargo test --locked -p openless-core provider
cargo clippy --locked -p openless-linux-egui --all-targets -- -D warnings
cargo test --locked -p openless-linux-egui --all-targets
cargo test --locked -p openless-linux-egui provider
cargo test --locked --manifest-path "src-tauri/backend-tests/Cargo.toml"
cargo check --locked --manifest-path "src-tauri/Cargo.toml" --lib
cargo test --locked --manifest-path "src-tauri/Cargo.toml" --lib
pwsh -NoProfile -File "scripts/check-command-event-baseline.ps1"
pwsh -NoProfile -File "scripts/check-core-deps.ps1" openless-core
pwsh -NoProfile -File "scripts/check-core-deps.ps1" openless-linux-egui
pwsh -NoProfile -File "scripts/check-core-secret-surface.ps1"
pwsh -NoProfile -File "scripts/check-core-test-isolation.ps1"
pwsh -NoProfile -File "scripts/check-core-runtime-seam.ps1"
pwsh -NoProfile -File "scripts/check-linux-public-surface.ps1"
node "scripts/shared-backend-wire-contract.test.mjs"
git diff --check
```

依赖检查命令预期无匹配；如果某个正常依赖间接拉入禁止包，必须先解决依赖方向，再增加 allowlist，不能把问题隐藏在脚本中。

**验收**

- core unit/integration、Tauri compatibility、Linux Adapter contract 和依赖门禁在 CI 中分别可见，失败时能定位到所属 Module。
- backend contract tests 直接依赖公开 crate/Interface，不再用 `#[path]` 或伪造 `AppHandle`
  绕过真实 package 关系；Tauri crate 的原测试数单独记录，不能再沿用旧“compatibility 118”数字。
- Windows 本地、macOS/Android cross-target 和真实 Ubuntu 原生验证分别记录；缺少某个平台证据时保持未完成状态。
- 所有质量门禁使用已提交 lockfile 和 `--locked`，避免验证时静默改写依赖解析结果。
- Core tests 并行执行时不共享持久化目录，结束后不在源码树留下 history/preferences/activity/style-pack 数据。

### M10：构建、打包、发布和文档收尾

**构建**

1. 保留 macOS / Windows Tauri 构建和签名路径。
2. 新建 Linux egui 构建 job，直接构建 `openless-linux-egui` binary。
3. Linux job 不安装 `libwebkit2gtk`，只安装 eframe/winit 实际需要的 X11/Wayland/音频/图形依赖。
4. Linux fcitx5 plugin 继续独立编译；主程序通过 host resource adapter 找到插件资源。
5. 根据最终打包工具生成 deb、rpm、AppImage；打包器不能重新引入 Tauri。
6. 产出独立的 Linux updater manifest、签名文件和 artifact 命名，避免与历史 Tauri Linux asset 混淆。

**工作流**

1. 把现有 `.github/workflows/release-tauri.yml` 的 Linux matrix 从 Tauri build 中移出，或拆成独立 `release-linux-egui.yml`。
2. macOS/Windows job 继续使用 Tauri cache 和 `src-tauri` manifest。
3. Android job 继续使用 Tauri mobile manifest。
4. Linux job 使用 Linux package manifest、独立 cache key 和独立 artifact path。
5. 更新 release notes、artifact 校验、updater endpoint 和安装说明。
6. 增加发布后验证：ELF 依赖、AppImage 内容、fcitx5 plugin 路径、桌面文件、单实例和 updater manifest。

**平台 runner 验证步骤（发布前必须逐项留证）**

| runner | 执行顺序 | 必须保存的证据 | 不能用来替代的证据 |
| --- | --- | --- | --- |
| Ubuntu 22.04 真实桌面 | 安装 X11/Wayland、PipeWire/ALSA、DBus、Secret Service、fcitx5 和打包工具；运行 Core/Linux contract；在真实登录会话中验证焦点输入、物理热键/translation 顺序、真实麦克风 start/stop、设置 effect、单实例转发/退出；再安装 deb/rpm/AppImage 做启动、升级、卸载 smoke | runner 日志、输入/音频设备信息、安装前后版本、包清单、ELF `ldd`、AppStream、签名和 updater SHA-256 | WSL 合成 DBus signal、无音频设备时的错误分类、临时 minisign、Windows Linux crate test |
| Windows | 先跑 frontend/Core/Tauri 全量门禁；再构建 MSVC Tauri artifact，执行 installer、启动/退出、真实 IME/插入、麦克风权限和 updater smoke；保留安装包、日志和校验值 | `cargo test --locked --manifest-path "src-tauri/Cargo.toml" --lib` 结果、artifact/installer、启动与升级日志、签名状态 | `cargo check` 或 Linux crate cross-platform contract 不能证明安装和原生输入 |
| macOS | 使用对应 SDK/Metal/ speech entitlement 构建 Tauri bundle；验证签名/notarization（如发布要求）、安装启动、NSPanel/Space、麦克风/插入和 updater；再运行 macOS 专属 contract | bundle/DMG、签名与 notarization 输出、真实窗口/输入日志、升级前后版本 | Windows 本地 Tauri test 或跨 target compile 不能证明 macOS native behavior |
| Android | 准备完整 JDK/SDK/NDK/Gradle cache；执行 `copy:android-scaffolding` 和全部 manifest/dependency merge 脚本；运行 `cargo ndk -t arm64-v8a check --manifest-path "src-tauri/Cargo.toml"`、`cargo ndk -t x86_64 check --manifest-path "src-tauri/Cargo.toml"`；执行 `npm run tauri:android:build:debug`（Windows shell 使用 `npm.cmd`）与 release/APK 分 ABI 构建；运行 Gradle JVM/unit、instrumentation、设备安装和 JNI/overlay/IME smoke；最后执行签名和产物校验 | Rust target、Gradle/JVM、APK/AAB、instrumentation、设备安装、签名和每 ABI SHA-256 | Rust cross-target check 不能替代 Gradle/APK、设备运行或签名证明 |

**文档**

- 更新 `openless-all/README.md` 和 `README.zh.md` 的平台说明、开发命令和 Linux 安装说明。
- 更新 `RELEASING.md`，区分 Tauri desktop、Android 和 Linux egui 发布流程。
- 新增 core API / Linux contract 文档，写明线程、事件、错误、能力降级和版本兼容策略。
- 删除“Linux 使用 Tauri”或“所有桌面平台共用 Tauri bundle”之类的过期描述。

**验收**

- macOS/Windows Tauri、Android Tauri mobile 与 Linux 原生宿主使用互相独立且可重复的构建入口。
- Linux 依赖树和最终 ELF/AppImage 中都没有 Tauri、WebKitGTK 或历史 WebView 运行时。
- deb、rpm、AppImage、desktop/AppStream metadata、fcitx5 资源、updater manifest、签名和校验值均由真实 Ubuntu runner 证明。
- 发布 job 在检测到 UI stub、缺失签名、ELF 依赖缺失或 contract version 不匹配时必须失败。
- 发布说明明确区分“Windows 本地 contract 通过”“跨 target CI 编译通过”和“真实 Linux 安装/运行通过”，三者不能互相替代。

**回退原则**

- 在 Linux 正式切换前保留最近一个已发布 Linux 产物和安装说明；不复用相同 artifact 名覆盖历史文件。
- 单个领域迁移失败时回退该领域的 Adapter 接线，不回退已经稳定的 core Interface 或其他领域。
- Tauri compatibility gate 失败时停止对应领域迁移；不得通过修改 React 调用方来掩盖无意的 IPC 破坏。
- Linux 原生验证失败时停止 Linux 发布，不影响 macOS/Windows/Android 的独立发布流程。

## 9. Tauri 适配器的命令迁移模板

迁移后的 Tauri command 应接近以下形状：

```rust
#[tauri::command]
async fn start_dictation(
    backend: State<'_, Arc<OpenLessBackend>>,
) -> Result<SessionIdDto, CommandError> {
    backend
        .start_dictation()
        .await
        .map(SessionIdDto::from)
        .map_err(CommandError::from)
}
```

不允许在 command wrapper 中：

- 判断 provider 优先级、重试、session phase 或 fallback 逻辑；
- 直接访问 `Coordinator` 的私有字段；
- 直接修改 preferences/history/vocabulary；
- 根据窗口 label 决定核心业务状态；
- 捕获错误后返回“看起来成功”的空结果。

事件桥接应集中在一个模块：

```rust
async fn forward_core_events(
    backend: Arc<OpenLessBackend>,
    app: AppHandle,
) {
    let mut events = backend.subscribe();
    while let Some(event) = events.recv().await {
        for mapped in map_event_for_react(event) {
            let _ = app.emit_to(mapped.target, mapped.name, mapped.payload);
        }
    }
}
```

`map_event_for_react` 是兼容层，不是业务层；Linux adapter 不应复用它。

## 10. Linux egui 团队接口手册要求

交付给 egui 团队的文档必须包含以下内容，缺一项就不能认为接口准备完成：

### 10.1 调用示例

- 构造 backend 的最小示例。
- 读取 startup snapshot。
- 非阻塞订阅事件并触发 egui repaint。
- 调用 settings/history/dictation/style pack 等领域接口。
- 取消运行中的 session。
- 正常关闭和异常关闭。

### 10.2 字段契约

- 每个 DTO 的字段、单位、默认值和 nullable 语义。
- 时间统一使用明确的毫秒/秒或 ISO-8601 规则。
- 音量 level 的范围固定为 `0..=1`。
- 流式 delta 的 session、sequence、offset 和最终态规则。
- `PlatformCapabilities` 每个字段在 Linux 不可用时的含义。

### 10.3 失败契约

- 权限未授权、provider 未配置、fcitx5 不存在、插入失败、下载失败、取消和超时的错误码。
- 哪些错误可重试，哪些错误需要用户操作。
- 哪些操作是幂等的：dismiss、cancel、shutdown、set enabled 等。
- outcome-unknown 时 UI 应等待 snapshot 或显示待确认状态，不能自行重复执行。

### 10.4 能力契约

至少覆盖：

| 能力 | core 字段 | Linux 可能状态 | UI 应看到的行为 |
| --- | --- | --- | --- |
| 全局热键 | `supports_desktop_hotkey` | available / unavailable | 隐藏或显示降级设置 |
| fcitx5 插入 | insertion capability | plugin missing / ready | 失败时提供 clipboard fallback |
| 托盘 | `supports_tray` | desktop / unavailable | 提供主窗口内替代入口 |
| 悬浮反馈 | host action | X11 / Wayland limitation | 不把窗口显示失败当作听写失败 |
| 本地 ASR | `supports_local_asr` | model absent / ready | 显示下载、准备和释放状态 |
| 自动更新 | `supports_auto_update` | package-dependent | 不显示假更新按钮 |
| 麦克风 | permission + device status | granted / denied / no device | 明确区分权限和设备 |

## 11. 风险与对策

| 风险 | 影响 | 对策 | 责任 |
| --- | --- | --- | --- |
| 搬迁时把 Tauri 类型带进 core | Linux 仍无法独立编译 | 依赖 grep 门禁；core 禁止 Tauri import | core 负责人 |
| 两套 UI 演化出两份业务规则 | 行为不一致、修复重复 | 业务判断只进 core；adapter 只翻译 | 全部 |
| 事件丢失或顺序错乱 | egui 显示旧状态、重复插入 | sequence + session + snapshot resync + lagged 测试 | core 负责人 |
| egui frame 被网络/磁盘阻塞 | Linux UI 卡死 | 事件 channel + 非阻塞 drain + repaint | Linux host / egui 团队 |
| Tauri command 仍包含业务逻辑 | Tauri 与 Linux 结果不同 | command contract review；wrapper 禁止业务分支 | Tauri 负责人 |
| Provider 验证/模型列表回流 Tauri | Linux 设置页无法复用 provider 管理面，凭据/协议出现第二份真相 | `ProviderService` 已迁入 Core；Core/Tauri/Linux contract + source contract；factory 断言非 `Unsupported` | core / Tauri / Linux host |
| Linux fcitx5 缺失 | 无法插入文字 | 明确 capability；clipboard fallback；不假成功 | Linux host |
| Wayland overlay/点击穿透限制 | 胶囊体验不完整 | 单独记录支持矩阵；不把 UI 反馈失败升级为 pipeline 失败 | Linux host / egui 团队 |
| 多 lockfile 发生依赖漂移 | 两个宿主可能使用不同传递版本 | 三份 lockfile 分别提交；CI 逐项使用 `--locked`，core Interface 由 path version + contract tests 约束 | 构建负责人 |
| Android cfg 被误删 | APK 回归 | Android job 和 Tauri mobile compile gate 保留 | Tauri 负责人 |
| 迁移测试仍 path include | 测试通过但实际 package 不可用 | backend-tests 直接依赖 core；删除 Tauri stub | 测试负责人 |
| secrets 泄露到 DTO/日志 | 安全事故 | status/value 分离；日志扫描和 fixture 检查 | core 负责人 |
| 大模型加载和下载被重复初始化 | 内存和启动时间回归 | backend 统一 runtime/cache 生命周期；增加资源计数测试 | core 负责人 |
| Core 私自创建 Tokio runtime | Linux egui 关闭/取消路径可能启动隐藏线程，生命周期和错误不可控 | 生产路径禁止 `Runtime::new()` fallback；实时 ASR 后台任务与关闭清理由宿主注入 `TaskSpawner` 提交；`scripts/check-core-runtime-seam.ps1` 扫描 `tokio::spawn`/`Handle::current`/`Runtime::new`，并在 CI 与 Linux release workflow 执行 | core 负责人 |
| 重复 session 覆盖已固定 provider | 返回 `Busy` 后原会话被错误接管 | registry 使用 entry/原子占位；回归测试验证旧路由仍可 finish/cancel | core 负责人 |
| fcitx5 translation modifier 信号晚于 dictation press | Linux 翻译模式与旧产品语义不一致 | Ubuntu 记录真实顺序；冻结关联窗口和时间线测试；不修改活动 session | Linux host 负责人 |

## 12. 验收标准

### 12.1 架构验收

- [x] core package 的源码和依赖树没有 Tauri、egui、eframe、WebView 类型。
- [x] Tauri 是 adapter，不再是 core 的隐式运行时；Core 只经显式依赖注入与 `TaskSpawner` 运行，Tauri command/source contract 不再保留已迁移领域的第二份业务实现。
- [x] `Coordinator` 不持有或接收 `AppHandle`/`WebviewWindow`，不直接 emit Tauri 事件；源码契约与残余引用检查已覆盖该规则。
- [x] core 事件不包含窗口 label 和前端事件名。
- [x] Linux package 不通过 path include 复用 Tauri 源码。
- [x] Android 仍能通过现有 Tauri mobile compile gate；CI runner 已验证 `aarch64`/`x86_64` Rust target、Gradle scaffolding、JVM/instrumentation 和 Keystore contract。
- [x] Core 生产异步路径不创建私有 Tokio runtime；实时 ASR 的后台任务和关闭清理由宿主注入
  `TaskSpawner` 提交，`check-core-runtime-seam.ps1` 已作为 no-private-runtime contract 在本地通过，
  并已加入 CI/Linux release workflow。

### 12.2 接口验收

- [x] `OpenLessBackend` 有构造、启动、快照、订阅、取消和关闭契约。
- [x] 领域 Interface 覆盖现有 React IPC 的有效业务分组：运行时 provider 与
  `ProviderApi::validate/list_models` 均由 Core `ProviderService` 实现，Tauri/Linux factory 共用同一
  service；其余未注入实现显式返回 `Unsupported`。
- [x] DTO serde 字段与现有 React IPC 的本地契约兼容；Local ASR 的 4 项 Tauri wire contract、Marketplace
  host sink 2 项、OAuth wire 2 项、Selection Core 17 项、Selection Voice Core 13 项、QA Core 15 项、
  Tauri QA Adapter 4 项和 Remote Input Core 8 项已通过；QA/Remote/Selection Voice 的 React source、
  Remote WebSocket lifecycle、Less Computer replay contract 及完整 frontend 58 项已通过；原生
  宿主行为证明由 12.4 的独立平台项约束，不与 DTO/serde 契约混算。
- [x] 错误码稳定，默认错误/事件序列化 fixture 不包含秘密字段；后续领域仍需继续执行敏感信息扫描。
- [x] event sequence、session guard、lagged resync 和终态唯一性有测试。
- [x] egui 团队的 1.0.0 领域 Interface、完整 headless 示例、mock、fixture、能力矩阵和 contract
  文档已具备；设置/快捷键 DTO、携带 snapshot revision 的 validated transaction、Linux settings
  runtime、Selection/Selection Voice 完整 headless 场景与 4 项 host contract 已进入移交基线，
  当前公共面门禁通过。该项只表示 Interface 移交完成，不包含 egui UI 或真实 Ubuntu 验收。

### 12.3 行为验收

- [x] core headless 测试覆盖听写成功、取消、ASR 失败、润色 fallback、插入 fallback 和 outcome-unknown。
- [ ] Tauri React 主链路、设置、历史、词典、风格包和 provider 页面可用。
- [x] Linux host 可通过 fake recorder/provider/inserter 调用同一 core Pipeline，且生产 factory 的
  provider validate/list_models 不经过 Tauri；Linux factory contract 已验证 provider service 非
  `Unsupported`。
  真实 Linux 设备与桌面集成仍由 Ubuntu 原生门禁证明，egui 视觉由另一组验收。
- [x] Linux fcitx5 可用/不可用、X11/Wayland、无托盘和无权限场景有 contract、fixture 与非 UI Adapter 测试。

### 12.4 构建和发布验收

- [x] Windows 本地 `cargo check -p openless-core`、`cargo check --manifest-path "src-tauri/Cargo.toml" --lib` 和 `cargo check -p openless-linux-egui --all-targets` 通过。
- [x] core 与 Linux package 依赖检查无 Tauri/WebKitGTK。
- [x] Tauri `cargo check --locked --lib` 在 legacy provider 副本清理后于当前工作树通过；既有/迁移期
  warning 不作为测试成功或跨平台原生证明。
- [x] 历史 Windows 本地 Tauri `cargo test --locked --lib` 为 745 passed、0 failed、7 ignored；远端
  macOS CI 在 run 33408317390 运行 737 项，其中 730 passed、7 ignored。Provider
  旧 command 测试旁路已删除，解析与模型响应测试归入 Core `ProviderService`；旧“backend
  compatibility 118 项”已由 Tauri 原 crate 测试取代，不再作为当前证据。
- [x] 历史 Windows 本地门禁记录 frontend build/58 项 tests、Core 594 项 unit + 79 项领域
  contract、Linux crate 29 项 + 4 项 host contract；远端 CI 的最新数字以 1.2.2 的
  `openless-core` 596 unit、Linux crate 30 tests 和 4 项 host contract 为准。
- [x] Core/Linux 严格 clippy、command/event baseline（196/30/29）、Core/Linux 依赖方向、secret
  surface、test isolation、Linux public surface、workspace fmt、provider command source contract、
  headless example 与 tracked `git diff --check` 均在当前工作树重跑通过；Core/Tauri/Linux provider
  管理面 contract 已通过。
- [x] Core runtime seam contract 已通过：生产源码不创建私有 `Runtime`，不直接调用 `tokio::spawn`；
  实时 ASR 的后台任务和关闭清理由宿主注入 `TaskSpawner` 提交，`check-core-runtime-seam.ps1`
  已加入 CI 与 Linux release workflow。
- [x] CI artifact 门禁已通过：run 33408317390 的 Linux runner 生成 deb/rpm/AppImage、fcitx5
  plugin 和独立 `latest-linux-egui-x86_64.json`，验证 ELF/包内容/desktop/AppStream 及
  manifest SHA-256；artifact `openless-linux-egui-x86_64`（ID `9764249814`）可下载，空
  `release_tag` 的 `minisign` 为 `null`，不会被误当作正式签名。
- [x] Core tests 使用每测试唯一且自动清理的临时目录；`check-core-test-isolation.ps1` 同时拒绝固定 crate-local `"data"` 和源码树运行残留。
- [x] Local ASR command 接线后的 core contract 6 项、Tauri wire contract 4 项和 Tauri
  `cargo check --lib` 通过；这只是定向证据，不能替代 Tauri 全量 tests 或其他平台证明。
- [x] Marketplace/OAuth 的 core contract 17 项、core 严格 clippy、Tauri host sink 2 项、OAuth
  wire 2 项和 Tauri `cargo check --lib` 已通过；本地全量门禁也已重跑。
- [x] 前序 Selection Core contract 17 项、Selection Voice Core contract 13 项和 Tauri Selection
  focused 22 项通过；Selection Voice Core/Tauri source contract 已覆盖业务边界；跨平台原生证明
  仍未完成，不能把定向 contract
  当作最新全量门禁或全领域迁移完成。
- [x] QA Core contract 15 项、Tauri QA Adapter 4 项与 Remote Input Core contract 8 项通过；QA/Remote
  lagged resync、Remote secret wire、Remote WebSocket lifecycle、Less Computer replay 和共享 React
  source contract 已由完整 frontend/Tauri suite 覆盖。跨平台原生证明仍未完成。
- [x] 快捷键迁移增量的 Core shortcut 5 项与 Tauri hotkey 14 项定向测试通过。
- [x] Core settings 成功/失败原子性、Linux validated 公共面与原生 effect substitute contract 已补；
  当前 fmt、Core/Linux clippy/test、frontend build/test、Tauri check/full test 与所有脚本门禁均通过。
  Android/macOS/Ubuntu 的真实 native effect 继续由对应未勾选项约束。
- [x] Provider validation/model-list Core/Tauri/Linux contract 已通过，且
  `commands/providers.rs` 不再包含 provider 协议、凭据读取或 HTTP/WS 请求构造。
- [ ] macOS/Windows Tauri artifact 的正式签名、完整安装/升级 smoke 仍未完成；run 33405500864
  已构建 macOS arm64/x86_64 DMG 和 Windows x64 NSIS，Windows NSIS 安装/卸载与 IME smoke 已通过，
  MSI 因 Beta.7 非数字版本按设计跳过。
- [x] Linux egui deb/rpm/AppImage、fcitx5 plugin、ELF 依赖、desktop/AppStream metadata 和临时
  minisign 签名/验签已在 WSL Ubuntu 验证；正式 updater manifest 仍需 release workflow 注入正式
  secret 后验证，UI stub 和正式发布门禁仍保持未完成。
- [x] README、RELEASING 和开发命令已区分 Tauri hosts 与 Linux egui host，并明确 UI stub 发布门禁。
- [x] Android CI debug artifact gate 已通过：run 33405500972 上传四个 ABI debug APK，
  `Collect split APKs` 校验每个 APK 只包含预期 ABI；这不包含 release 签名、设备运行或安装证明。
- [ ] Android release 签名、设备安装/升级和 JNI/overlay/IME 真实 smoke；本机 `cargo ndk` 的
  `arm64-v8a` 与 `x86_64` Rust cross-target check 不能替代这些证明。
- [ ] Ubuntu 真实桌面 runner 完成焦点输入、fcitx5 物理按键顺序、真实音频设备 start/stop、设置
  effect、单实例退出和安装后启动；WSL 合成 signal、无设备错误和临时签名均不能替代该证明。
- [ ] Linux 正式签名密钥注入后的 updater manifest、artifact 校验和、安装/卸载与回滚验证。

## 13. M0 决策记录

以下事项已经冻结；若要改变，必须同步更新 contract version、fixtures 和两个宿主：

1. `openless-core` 的 Interface 覆盖现有有效业务领域；egui UI 可以分阶段展示，但不能复制或绕过 core 规则。
2. Linux deb/rpm 使用 `fpm`，AppImage 使用 `appimagetool`，产物由独立 workflow 生成；打包器不得引入 Tauri/WebKitGTK。
3. runtime 由宿主提供，core 只依赖注入的 `TaskSpawner`；当前默认 Adapter 为 `TokioTaskSpawner`。
4. `BackendEvent` 使用有界 `tokio::broadcast`，落后订阅者收到显式 `Lagged` 并从 snapshot/query 重同步。
5. core 使用 `DictationStateSnapshot` 等语义名称；`CapsulePayload` 等旧名和窗口 payload 只存在于 Tauri compatibility Adapter。
6. Linux tray/autostart/overlay/updater 都是 capability；不可用时 UI 隐藏或降级，不能伪造支持。
7. `BACKEND_CONTRACT_VERSION` 独立管理 Interface 破坏性变更；应用发布版本仍由宿主产物共同决定。
8. Linux 发布 workflow 在真实 egui 入口替换 stub 前不响应 tag；即使脚本能生成包，也不能把 stub 标为正式发布。

## 14. 参考资料

### 仓库内依据

- [`openless-all/app/src-tauri/Cargo.toml`](../openless-all/app/src-tauri/Cargo.toml)
- [`openless-all/app/src-tauri/src/lib.rs`](../openless-all/app/src-tauri/src/lib.rs)
- [`openless-all/app/src-tauri/src/coordinator.rs`](../openless-all/app/src-tauri/src/coordinator.rs)
- [`openless-all/app/src-tauri/src/commands/mod.rs`](../openless-all/app/src-tauri/src/commands/mod.rs)
- [`openless-all/app/src-tauri/src/types.rs`](../openless-all/app/src-tauri/src/types.rs)
- [`openless-all/app/src-tauri/backend-tests/tests/core_contract.rs`](../openless-all/app/src-tauri/backend-tests/tests/core_contract.rs)
- [`openless-all/app/crates/openless-core/src/provider_registry.rs`](../openless-all/app/crates/openless-core/src/provider_registry.rs)
- [`openless-all/app/linux-egui/src/runtime.rs`](../openless-all/app/linux-egui/src/runtime.rs)
- [`openless-all/app/src/lib/ipc/index.ts`](../openless-all/app/src/lib/ipc/index.ts)
- [`release-tauri.yml`](../.github/workflows/release-tauri.yml)

### 稳定的上游资料

- [Tauri Architecture](https://v2.tauri.app/concept/architecture/)
- [Cargo Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [eframe API](https://docs.rs/eframe/latest/eframe/)
- [egui API](https://docs.rs/egui/latest/egui/)

这些资料只用于确认宿主/库的常见职责和 Cargo workspace 组织方式；项目最终接口以本文的仓库事实、contract tests 和实际构建结果为准。

## 附录 A：本次接口准备的实际交付

当前已落地、可供 egui 组开始 view model 和 headless 集成的内容：

- `openless-core`：无 Tauri/egui 依赖的 facade、快照、错误码、语义事件、宿主 Interfaces、
  听写状态机、共享 `PipelineDictationEngine`、16 kHz mono Int16LE PCM/WAV 契约、
  `start/finish/cancel` engine Interface、录音 level/阶段/增量 progress sink、非阻塞事件订阅
  和取消/迟到结果/生命周期测试；
- 设置/快捷键：语法、左右修饰键、legacy 同步、跨功能冲突、strict/reconcile、preserve-style、
  revision guard、显式 effect plan、receipt 补偿和单写入 gate 已进入 core；完整设置 DTO 与
  `LinuxHost::save_settings`/`update_settings_strict` 已从 Linux contract 交付。Core settings 11 项、
  Linux 4 项公共 host contract 和本地全量门禁通过；真实原生 listener 事务仍由对应 runner 证明；
- 词典与 style-pack 诊断：`enabled_vocabulary_phrases`、`asr_vocabulary_phrases` 和
  `preview_style_pack_runtime` 均由 Core facade 提供；ASR 热词保底/命中排序/大小写去重及
  prompt 诊断不再由 Tauri Coordinator 复制，Tauri command 只做 Core/wire 转换；
- `openless-linux-egui`：只依赖 core 的 Linux host seam 和无 UI 的
  `examples/headless_host.rs`；
- `openless_core::testing`：记录 host action、fixture recorder/transcription/polisher/engine/inserter/
  selection 和固定结果/错误的 headless 测试替身；inserter 记录 session-scoped
  prepare/insert/cancel 顺序，selection fixture 记录 capture/preview/apply/revert/cancel 并可表达
  Linux preview/revert `Unsupported`；
- `LinuxCapabilityFixture`：X11 完整、Wayland 降级和 headless 能力/权限快照；
- `scripts/check-core-deps.ps1`：core/Linux package 的禁止依赖门禁；
- `scripts/check-core-runtime-seam.ps1`：禁止 Core 生产路径创建私有 Tokio runtime 或直接 spawn，
  并确认后台任务经过宿主注入的 `TaskSpawner`；
- [`linux-egui-backend-contract.md`](./linux-egui-backend-contract.md)：当前可用接口、
  事件/错误/能力契约和未完成领域的明确边界；
- `BackendServices`：provider/local ASR/selection/QA/remote input/marketplace/
  coding-agent/platform/auxiliary 的稳定 Interface 与 DTO；未注入 Adapter 时统一失败为
  `Unsupported`；`AuxiliaryApi` 额外交付单轮 repolish、规范 PCM retranscription、实际 ASR
  provider/model 归因、terminal Foundry fallback 和 future-drop cancel 契约；
- Provider 管理面已完成迁移：云端 provider 运行时及
  `ProviderApi::validate/list_models` 均由 Core `ProviderService` 共享实现承载，Tauri
  `commands/providers.rs` 只做参数/旧 wire/error 转换，Linux 生产 factory 通过
  `from_shared_providers` 注入同一 service；`ProviderTransport`、fake transport 覆盖、静态模型
  parity 与 LLM 显式 channel 写入均已有回归测试。egui 可以直接调用公开 provider Interface；真实
  网络、keyring 和平台 runner 仍须按 M9/M10 留证，不得用 fixture 冒充生产能力。
- Coding Agent：provider/model/权限/预算/路径/风险/版本/MCP 解析等跨宿主规则位于 core；
  Tauri 使用真实 `TauriCodingAgentApi` 处理 CLI 进程、Git 快照、临时 guard 配置、审批和 typed
  event 转发，commands 只保留主窗口授权与旧 React wire 转换；Linux 可直接复用同一 Interface，
  未提供进程 Adapter 时稳定返回 `Unsupported`；
- Less Computer 语音生命周期：Core 提供实例级 capture lease、active session、取消可见性和
  幂等 abort；Tauri 热键按下先预留 lease，再以同一 session id 驱动兼容 recorder/ASR，转录后
  通过 `submit_less_computer_with_session` 进入 Core Agent run；Esc/启动失败/空转写不会遗留
  capture lease。egui 只消费这些 facade 与 typed events，不读取 Coordinator 状态；
- Local ASR：Generic/Foundry/Sherpa 的 catalog、设置事务、生命周期 Interface、engine-changed
  事件语义与共享 `LocalAsrService` 位于 core；Tauri 使用 `LocalAsrRuntimeAdapter` 承担原生引擎、
  下载和文件操作，三组 command 只保留旧参数/DTO 转换，Generic 下载已通过 typed event 进入
  集中桥接，未知 Sherpa family/mode 使用 fallible conversion 返回错误；Adapter 直接注入共享
  preferences repository 与 native cache，不再通过 `AppHandle` 回取 Coordinator；Coordinator 的
  ASR 就绪门禁也消费同一 Core 偏好快照，不再重新打开第二份 preferences store；
- Marketplace：完整 `MarketplaceApi`、HTTP/认证策略、归档大小与 ZIP 校验、安装事务、
  upload/origin 写回、实例级 OAuth device-flow registry、401 tombstone 和 secret-surface 规则已
  进入 core；Tauri Marketplace/OAuth commands 只保留旧 wire/error 转换与归档最终写入；Linux
  生产 factory 已接线 Marketplace，`LinuxHost::download_marketplace_archive` 以 create-new 语义把
  已校验归档写入绝对 filesystem path，拒绝覆盖并清理失败写入；
- Selection：Core Implementation 已拥有 preview/confirm/direct apply/cancel/revert 状态、typed
  event、provider/context 冻结、history/vocabulary 写入、迟到结果与 outcome-unknown 语义；17 项
  Selection contract 与 13 项 Selection Voice contract 当前通过。fixture、headless 示例和 Linux
  host contract 已覆盖 preview/confirm/cancel/stale/outcome-unknown 以及 Linux preview/revert
  `Unsupported`。Tauri 已注入新的 runtime 和共享 polisher，旧 `TauriSelectionApi`/Coordinator
  wrapper 已删除；Selection Voice 的 correction/instruction/intent/output-mode/EditPlan/translation
  和 QA preview revision 已由 Core 高层 use-case 统一，Tauri 只保留 native recorder/window/hotkey、
  opaque insertion target 与 apply outcome；跨平台原生验证仍待收口；
- QA：`QaService` 已拥有 message log、phase、text/voice turn、selection envelope、level/delta、
  approval token、cancel/dismiss、错误脱敏、`ShowQa` 失败回滚和 shutdown 语义，15 项 contract 通过；生产构造已注入
  `TauriQaRuntimeAdapter`，QA hotkey/commands/dismiss 与 Selection Voice 问答/编辑预览均调用同一
  `QaApi`，独立 `QaHostState` 已删除，Coordinator/QA Adapter 共享一个窄 `TauriQaHostContext`，
  Tauri QA Adapter 4 项通过；Less Computer 已实现 listener-first replay、同步期 pending
  合并、sequence 去重与截断重建；原生平台证明仍待收口；
- Remote Input：`RemoteInputService` 已拥有配置、PIN、locale、连接/session、PCM 校验、事件与
  shutdown，8 项 contract 通过；Tauri TLS/WSS/PIN 文件与 external dictation 位于 runtime Adapter，
  Coordinator 已删除重复状态；WebSocket contract 覆盖认证顺序、constant-time PIN、单 stream、
  disconnect/restart cancel 和 stale lease，`RecordingRemoteInputRuntime` 可供 headless 测试；真实
  WSS/证书/防火墙原生网络证明仍待补；
- Linux Adapter：Secret Service/keyring 凭据、非秘密 metadata、资源布局、fcitx5 DBus 与插件
  安装契约、X11/Wayland/headless 能力、cpal 设备枚举与录音、DBus 热键 listener、
  `LinuxBackendBuilder`、HostActions 和 Unix socket 单实例 intent 转发；新增 3 个显式 ignored
  native contract，已在 WSL Ubuntu 分别验证 Secret Service set/read/remove、fcitx5 plugin/method/
  listener/signal 映射和 cpal 无设备错误分类；无焦点输入时 fcitx5 plugin 不再抛异常导致宿主崩溃；
- `src-tauri` 已添加对 core 的 path dependency 并复用同一组 repository；云 ASR/LLM/Omni、
  Auxiliary、QA provider 运行时及 provider validation/model-list 管理面均改用 Core 共享实现；
  Tauri provider commands 只保留旧参数/DTO/error 转换。Tauri 仍提供
  `SystemCredentialStore`、平台录音、native/local ASR、窗口/插入与 runtime；旧 Coordinator 仍按
  M4–M6 收窄，原 12 个 `migrationRequired` 事件已全部进入集中桥接；
  React command、CLI、Android JNI、remote PCM 和桌面普通听写的主要热键边沿已切 core；QA/shortcut/combo/
  debounce 等宿主仲裁仍留在 Tauri。Less Computer 文字入口、capture lease、同 session submit/cancel
  已接线；其语音按下/松开、Starting pending stop 和静音自动停止仍使用 Coordinator 的兼容 host
  session 状态，但该状态不再承载 Agent 业务规则，也不能被 Linux egui 读取。`Coordinator::Inner` 与 `capsule_focus` 已恢复 module 私有，
  `bind_app(AppHandle)` 已删除，Coordinator/capsule 子模块不再出现 `AppHandle`、`WebviewWindow`、
  直接 emit 或直接 `tauri::async_runtime`；capsule 原生窗口操作和 layout/cursor/style/fallback/
  deferred cache 已移入 `TauriCoordinatorHost`，payload 应用只接收窄值。compatibility Coordinator
  仍持有显式 Host，并承担部分热键仲裁、native runtime 生命周期和兼容编排，需要继续按生产调用图
  收窄；`core_adapters.rs` 已无 `managed_coordinator` 反向查询，hotkey/QA 状态通过窄共享依赖注入；
- core facade 在插入、结果、history 和 activity 之前统一应用启用的最终纠正规则；traditional
  history 使用冻结的 ASR/LLM channel 与 model，multimodal history 清空 ASR 归因并记录冻结的
  Omni channel/model；
- provider router 明确区分 `provider_id`（channel/scoped credential）与 `provider_type`
  （协议路由），并冻结 session 的 provider ID/type/model；Tauri 生产 Adapter 的重复 session
  使用原子占位，不会在返回 `Busy` 时覆盖原取消路由。
- Linux 生产 UI 只调用 `LinuxBackendBuilder::from_shared_providers(config)`；Core 共享
  ASR/LLM/Omni/Auxiliary、ProviderService、Linux credentials、cpal recorder、fcitx5 inserter 与
  settings runtime 由 factory 内部组装。`LinuxBackendBuilder::new(...)` 的显式 provider 注入只用于
  测试/特殊宿主；仅 native/local ASR 等尚未提供 runtime 的能力可以返回 `Unsupported`。

补充本地与远端全量证据：历史 Windows 本地记录 frontend build 与 58 项 frontend/contract tests、
Core 594 项 unit、Linux Adapter 29 项 crate tests + 4 项 host contract、Tauri 745
passed/0 failed/7 ignored；最新 fork CI run 33408317390（head `06e85f7b`）记录 Core 596 unit、
Linux Adapter 30 tests + 4 host contract、macOS Tauri 730 passed/0 failed/7 ignored，并通过
workspace fmt、Core/Linux 严格 clippy、测试隔离、公共接口、command/event baseline（196/30/29）、
依赖方向、secret surface、source contract、headless example、Provider command 禁回流 contract
与 tracked `git diff --check`。已删除 path-include suite 的 118 项数字不再作为证据。
Windows 上的 Linux package test 只证明跨平台 Rust contract，不证明 DBus/Secret
Service/cpal/fcitx5 的真实 Linux 行为；Android、macOS、Ubuntu 打包与原生集成必须由对应
runner 证明，不能从本机结果推断。

### 2.0.0-Beta.1 版本与许可证边界

本批 Tauri 应用版本统一为 `2.0.0-Beta.1`；`BACKEND_CONTRACT_VERSION` 已升级为
`2.0.0`，应用版本和接口版本仍各自独立。根项目从该版本起采用 `AGPL-3.0-only`，已发布 1.x 版本仍保持 MIT，
第三方 vendor 文件保留其原始 MIT/Apache/LGPL 条款。Less Computer 语音 session、实时
`TranscriptDelta` 和 Linux 三档热键事件已进入 Core/Host contract；真实设备、签名和 UI
验收继续按 M8–M10 单独取证。
