# Linux egui 后端接口契约（2.0.0）

> **2026-09-06更新**：接手请先读[Linux交接目录](./linux-egui-handoff/README.md)中的短文档；本文保留长接口与历史实现参考，不再作为唯一交接材料。
> 当前范围以[2.0需求](./2.0-requirements.md)为准：Windows/macOS完整保留各自Tauri 1.x功能；Linux交付可接入Core。egui团队承接剩余Linux Host原生接线、UI、设备与发布验收，不仅负责绘制；Linux产品未完成不单独阻塞本批Windows/macOS交付。
>
> 2026-09-04 当前状态：2.0.0 facade、repository、Provider/Credential、听写 Pipeline、Local ASR
> 原子激活、主热键仲裁、录音生命周期、上下文/手改观察、静默重试、插入 reconciliation、
> Transcript reducer、Less Computer/Coding Agent 以及全部领域 Interface 均由 Core 持有。
> Tauri 与 Linux 共用云 ASR/LLM/Omni/Auxiliary Implementation；平台 Host 只注入原生录音、
> 凭据、窗口、进程、焦点和插入 Adapter。Linux production factory 已注入真实 QA、Remote Input、
> Selection preview/revert 与打包的 Qwen runtime；AppImage 会在 listener 前安装 fcitx5 插件。
> 自动 contract 不替代真实平台证据：Ubuntu/Windows/macOS/Android 的设备、安装、升级和签名
> 结果仍须单独记录，当前责任与门槛按[桌面验收](./2.0-desktop-acceptance.md)和[Linux验收](./linux-egui-handoff/07-acceptance.md)区分；以下旧日期状态不能证明Linux无剩余缺口。
>
> 2026-09-05 复核补充：最新生产行为修复见 [`pr1019-2.0-final-review.md`](./pr1019-2.0-final-review.md)。
> 原生预加载必须使用请求的 target/provider type；平台无法准备流式时以 `supports_streaming=false`
> 保留一次性落字。Remote stop 保持可取消的 session，socket 下行只转发本连接所属 session 的事件。

## 1. 依赖与职责

Linux UI 只能依赖 `openless-linux-egui -> openless-core`。`openless-core` 的正常
依赖树不得出现 `tauri`、`wry`、`webkit2gtk`、`egui` 或 `eframe`；Tauri 只能在
`src-tauri` adapter 中出现。

```toml
[dependencies]
openless-core = { path = "../crates/openless-core" }
```

核心拥有业务状态、会话、provider 选择、持久化和语义事件。Linux host 拥有
Tokio runtime、窗口/托盘、fcitx5、麦克风、凭据和系统操作。egui frame 只读取
快照、非阻塞地 drain 事件，并在事件到达后请求重绘。

## 2. 构造与生命周期

核心构造不创建窗口、不申请权限、不注册全局热键，也不启动网络任务。宿主按
以下顺序初始化：

1. 解析 `data_dir`、`cache_dir`、资源目录、locale 和 `PlatformCapabilities`。
2. 创建各 platform port，组装 `BackendDependencies`。
3. 调用 `OpenLessBackend::new`。
4. 创建事件订阅和 host action sink。
5. 调用 `start()`，把返回的 `StartupSnapshot` 放入 UI view model。
6. 只有启动成功后才启用热键、设备 watcher 和下载 watcher。

Linux 生产宿主不自行组装 provider，也不读取 credential account。以下生产 factory 必须在Host已有的Tokio runtime上下文中调用；同步GUI初始化用短作用域`runtime.enter()`包住构造，随后退出作用域再启动运行时任务。具体executor生命周期与自定义注入约定见[当前接入说明](./linux-egui-handoff/01-core-contract.md)。

```rust
let runtime = LinuxBackendBuilder::from_shared_providers(config)?.build()?;
let backend = Arc::clone(&runtime.backend);
```

`LinuxBackendBuilder::new(config, transcription, polisher)` 仅供测试或特殊宿主显式替换 provider；
egui UI 不应调用它，也不应注册 router 或解析 endpoint/model/extra headers/temperature。

关闭时先禁止新会话，取消活动任务，停止 host watcher，flush 必要持久化，停止
事件桥，再调用幂等的 `shutdown()`。

```rust
let backend = Arc::new(OpenLessBackend::new(config, dependencies)?);
let mut events = backend.subscribe();
let startup = backend.start().await?;
let _session = backend.start_dictation().await?;
let result = backend.stop_dictation().await?;
backend.shutdown().await?;
```

`start()` 和 `shutdown()` 可重复调用；重复启动不会重复发布
`BackendStarted`，重复关闭不会 panic。`snapshot()` 是同步、无副作用的 owned
读取，不返回内部锁或引用：

```rust
let current: BackendSnapshot = backend.snapshot();
```

## 3. 当前公开接口

### 3.1 Facade

| 方法 | 线程/等待 | 语义 |
| --- | --- | --- |
| `new(config, deps)` | 同步 | 校验配置并注入依赖；不启动副作用 |
| `start()` | async | 初始化核心生命周期；幂等 |
| `shutdown()` | async | 停止核心生命周期；幂等 |
| `snapshot()` | 同步 | 返回可克隆快照 |
| `subscribe()` | 同步 | 创建独立事件订阅 |
| `start_dictation()` | async | 建立唯一活动 session，`Starting` 成功后进入 `Recording` |
| `stop_dictation()` | async | `Transcribing -> Polishing -> Inserting -> Completed`，执行 ASR/润色和插入 |
| `cancel_dictation(session)` | async | 取消指定或当前 session；session 不匹配时报错 |

同一时刻最多一个 dictation session。UI 不应通过按钮状态猜测可用性；调用仍需
处理 `Busy`、`InvalidState` 和 `Cancelled`。

### 3.2 已建立的领域 Interface

数据领域直接由深 facade 提供，所有宿主共用同一份 repository 和规则：

| 分组 | 主要操作 |
| --- | --- |
| preferences | `get_preferences`；完整文档和单项更新统一通过 `LinuxHost` 的 validated settings Interface |
| credentials/channels | status、显式 `SecretValue` read/write/remove、channel CRUD/reorder/test、active provider |
| history/activity | list/recent/append/update/delete/clear、activity snapshot/bump |
| vocabulary/correction | list/add/remove/enable/hits、preset、correction-rule lifecycle；`enabled_vocabulary_phrases()` 与 `asr_vocabulary_phrases()` 返回 Core 过滤/排序后的 owned 词条 |
| style packs | list/get/create/update/activate/enable/reset/delete、安全 ZIP import/export；`preview_style_pack_runtime(style_pack)` 返回由 Core 统一组装的单轮/多轮 prompt 诊断 |
| dictation | start/stop/cancel、snapshot、session-scoped progress 与插入终态 |

有平台、网络、进程或 runtime 变化点的复杂领域通过
`OpenLessBackend::services() -> &BackendServices` 暴露：

| Interface | 已冻结的 use-case |
| --- | --- |
| `ProviderApi` | validate、list models，按 ASR/LLM/Omni 和可选 channel 选择 |
| `LocalAsrApi` | settings/catalog/status/remote info、目录/模型/镜像/keep-loaded 设置、download/prepare/preload/release/delete/test |
| `ModelStore` | Core 统一 catalog、HF tree 分页、文件过滤、Range/断点下载、SHA-256、staging/ready sentinel、归档解压与 1.x 模型目录迁移 |
| `SelectionApi` | snapshot、begin polish、confirm、cancel、revert |
| `SelectionVoiceApi` | begin/mark processing、Core-owned transcript 处理、intent confirm、edit delivery、QA preview create/revise、preview apply ticket/finish、cancel/revert |
| `QaApi` | snapshot、toggle recording、按每轮token stop_recording、submit text、edit-instruction mode、session cancel、dismiss |
| `LessComputerApi` | submit、cancel、dismiss、begin turn、approval decision；Core 统一 provider/model/permission/workdir/prompt/guard/continuation |
| `RemoteInputApi` | 同步 status、configure、显式读取/重新生成 pairing PIN、locale、local IPs、connect/disconnect、start/feed/stop/cancel stream |
| `MarketplaceApi` | list/detail/install/download/upload/like/delete、my lists、GitHub device flow、logout |
| `CodingAgentApi` | detect/list models/risk/run/cancel/approve |
| `PlatformApi` | microphone devices/permission、accessibility permission、permission request、hotkey status |
| `AuxiliaryApi` | 对既有文本执行单轮 repolish；对宿主提供的规范 PCM 执行单轮 retranscription |

`BackendServices::unsupported()` 是正式的降级 Adapter：每个调用返回
`BackendErrorCode::Unsupported`，不会启动 task 或返回空成功。egui crate 不得直接 include
`src-tauri/src/*.rs`，也不得为了暂时可用而复制业务逻辑。

`preview_style_pack_runtime(style_pack)` 是同步、无 I/O 的 Core 查询。它读取当前偏好和已启用
词典，使用与生产润色路径相同的 prompt composer，返回 `StylePackRuntimeDiagnostics`（包括
单轮/多轮 prompt、上下文 premise、热词块及字符数）。宿主只渲染返回 DTO；不得在 Tauri 或
egui 中重新拼接 prompt、过滤热词或推导字符数。

### 3.3 `ProviderApi`

`ProviderApi::validate` 与 `list_models` 由 Core `ProviderService` 统一实现。Tauri
`commands/providers.rs` 只负责 `kind` 字符串解析、请求构造和旧错误字符串转换；Linux
生产入口 `LinuxBackendBuilder::from_shared_providers` 注入同一 service，egui 不读取凭据或
构造 HTTP/WS client。

- `ProviderRequest { kind, channel_id }`：`channel_id = None` 使用该类别的 active provider；显式
  channel 必须存在于 metadata。Omni 不接受 channel id，返回 `InvalidArgument`。
- channel 的 `provider_type` 来自 credential metadata，不能用 channel id 猜协议；凭据通过
  `CredentialKey { namespace, provider_id, account }` 读取，A/B channel 不会串线。
- `validate` 使用与正式 ASR/LLM/Omni 调用相同的 Core provider 构造路径；静音探活不保存用户音频。
- `list_models` 对 Codex、Bailian、Qwen、Mimo、ElevenLabs 等无远端列表接口返回 Core 静态清单；
  OpenAI-compatible 与 Gemini 使用受大小上限约束的远端列表响应，并校验 JSON schema。
- 参数/模型缺失返回 `InvalidArgument`，凭据或网络/HTTP/WS 失败返回 `Provider`，超时/连接失败
  标记 `retryable`，取消返回 `Cancelled`，native/未知 provider 返回 `Unsupported`。
- 模型响应最大 2 MiB；状态码只以 `providerHttpStatus:<code>` 形式返回。API key、token、Authorization、
  endpoint credential 和响应 body 不进入 DTO、错误详情、日志或 `Debug`。

Core、Tauri wire 和 Linux factory contract 覆盖 channel 隔离、Omni 拒绝、静态/远端模型解析、
错误脱敏和非 `Unsupported` 生产接线。未注入的其他领域仍稳定返回 `Unsupported`。

当前 provider 管理面已可供 egui 调用。模型列表请求通过 Core `ProviderTransport` seam；生产实现使用
无 redirect、显式 timeout 和 2 MiB 响应上限，`FakeProviderTransport` 已覆盖 HTTP 状态、超时、取消、
无效 JSON、响应过大和 redirect 拒绝，并验证秘密不会出现在请求 `Debug`、错误或 fixture 输出。Core
静态模型清单已按迁移前 Tauri 顺序完成 provider parity 与去重测试；LLM extra headers/temperature
写入已按显式 `provider_id` 定位并有 A/B channel 回归测试。上述 Interface 收口升级为 2.0.0 公共调用面，
但真实 provider 网络、keyring/Secret Service、取消/超时和各平台 runner 仍需在 M9/M10 留下独立证据，
不能用 fake、WSL 或本地 parser/unit 测试代替。

Less Computer 文字入口可直接提交；语音/物理热键入口必须先建立 Core capture lease，
再把同一个 session id 贯穿宿主录音、取消和提交：

```rust
let result = backend.submit_less_computer("列出当前目录的文件".to_string()).await?;

let session_id = SessionId::new();
backend.begin_less_computer_capture(session_id)?;
// Host starts its recorder/native ASR. Startup failure must release the lease:
// backend.abort_less_computer_capture(session_id)?;

// 录音热键完成转写后，使用同一 session 以便 Esc/release 精确取消：
let result = backend
    .submit_less_computer_with_session(session_id, transcript)
    .await?;

// Esc/cancel must signal Core before the host drops recorder/ASR resources.
backend.cancel_less_computer(Some(session_id)).await?;
// If submission has not promoted the lease to a run, release the capture lease:
backend.abort_less_computer_capture(session_id)?;
```

两个 submit 方法都只接受用户文本；provider、executable、model、permission mode、workdir、
autonomous prompt、命令护栏、审批重跑和 dsh continuation 均由 Core 从 preferences 与实例状态
解析。`begin_less_computer_capture` 是实例级、session-scoped 的互斥 lease；重复 reserve 返回
`Busy`，`less_computer_active_session` 可用于重连/诊断，`less_computer_capture_cancelled` 在
宿主释放 capture lease 前保持可见。`abort_less_computer_capture` 只释放仍处于 capture 阶段的
lease，对已提升为 Agent run 的 session 是幂等 no-op。`LessComputerRuntimeAdapter` 只负责宿主
进程/Git/临时文件/stream transport；未注入 runtime 时返回 `Unsupported`。同一实例同时提交
第二轮返回 `Busy`，不能覆盖当前运行。

返回的 `LessComputerRunResult` 只表达该轮唯一终态：
`Completed { text, cost_usd }`、`Failed { message }` 或 `Cancelled`。UI 不应根据 delta、窗口关闭
或进程退出自行猜测终态；必须等待对应的 `LessComputerEvent`。

Linux 生产 factory 会注入 Core `MarketplaceApi`。需要把归档保存到用户文件系统时，UI 调用
`LinuxHost::download_marketplace_archive(pack_id, target)`：Core 负责 HTTP/OAuth/大小限制与 ZIP
校验，Linux host 只接受已有父目录下的绝对路径，以 create-new 方式写入并拒绝覆盖；写入或
`sync_all` 失败时删除不完整文件。UI 不处理 bearer token，也不自行重复校验归档。

### 3.3 `AuxiliaryApi`

`AuxiliaryApi` 是“重新润色”和“从既有录音重新转写”的共享 use-case，不拥有文件选择、
WAV 解码、窗口或进度 UI：宿主先把输入转换为契约数据，再调用 Core。

- `repolish(RepolishRequest)` 接收 owned `raw_text`、可选 `style_pack_id` 和可选
  `front_app`。Core 在调用开始时冻结 preferences、指定或当前 style pack、启用词典以及
  LLM/Omni provider；它不解析 ASR provider。该方法只执行一轮，不写 history/activity、
  不插入文本、不改变 active style pack，也不发流式 delta。当前模式不使用 polisher 时，
  原文原样返回。
- `retranscribe_pcm(Vec<u8>)` 只接受非空、偶数字节的 16 kHz mono signed Int16
  little-endian PCM；文件读取、WAV header 去除和格式转换归宿主。Core 只冻结 ASR provider，
  不要求 LLM/Omni 凭据，且把全部 PCM 精确送入一个 `TranscriptionSession` 后只 finalize 一次。
  成功返回 `RetranscriptionResult { text, duration_ms, asr }`；`asr` 是 Adapter 实际使用的
  provider/model（包含默认模型解析后的值），不是 UI 提交值。
- 失败返回 `RetranscriptionFailure { error, attempted_asr }`。ASR session 尚未建立时
  `attempted_asr` 可为 `None`；建立后必须携带实际归因。Foundry 的 terminal fallback 使用
  `details.terminal = "foundry_fallback"`，`is_terminal() == true` 且 `retryable == false`；
  其他 transcription 启动/finalize 失败统一标为可重试。
- 如果调用方 future 在 finalize 完成前被丢弃，Core 的取消 guard 通过宿主注入的
  `TaskSpawner` 调用该 session 的幂等 `cancel()`；宿主不能对同一 PCM 自动启动第二次转写。

Linux 生产 factory 已注入共享 Auxiliary polisher 与 transcription router；它与 Tauri 使用同一套
provider 选择、凭据路由、默认值、取消和错误语义。native/local ASR 仍需 Linux 对应 runtime；
未注册的 native/local provider 稳定返回 `Unsupported`，不能把 headless fixture 当作真实能力。

### 3.4 `SelectionVoiceApi`

Selection Voice 的业务入口是高层 use-case，不是让宿主拼装 prompt 或 EditPlan 的工具箱：

- 宿主完成平台录音/ASR 后，先调用 `mark_processing(session_id)`，再把未加工的 transcript 交给
  `process_transcript(session_id, transcript)`。Core 依次执行 correction rules、指令润色、按冻结偏好
  选择 manual/heuristic/prompt/auto intent；auto 模型失败时只由 Core 回退 heuristic。宿主不得预先
  润色、分类或传入模型分类结果。
- `SelectionVoiceDisposition::Question` 只要求宿主打开 QA surface、提交 Core 返回的 instruction，
  成功后调用 `complete`；`AwaitingIntent` 只要求显示 prompt 并把用户选择交给 `confirm_intent`。
- `SelectionVoiceDisposition::Edit` 后调用 `prepare_edit(session_id, owner)`。Core 根据
  `selection_polish_output_mode` 返回 `SelectionVoiceEditAction::OpenConversation`，或在内部完成
  translation target 推断、provider 调用、EditPlan 解析/应用后返回 `ReadyToApply { preview }`。
- QA 编辑模式调用 `edit_preview(SelectionVoiceEditRequest)`。首次调用建立或复用匹配的 Core session；
  后续调用以当前 preview 为 draft，只保留一步 revert。`replaced_existing` 是 QA 按钮状态的唯一真相，
  `answer_text()` 是稳定的 assistant message 投影；Adapter 不应重复格式化 summary。
- 真正替换文本仍是双阶段握手：Core `begin_preview_apply` 返回 ticket，平台 Adapter 用 opaque target
  校验/插入，再以 `finish_preview_apply(ticket_id, outcome)` 回报。只有 `Inserted` 或
  `CopiedFallback` 才会消费 preview 并写 history/activity；`Failed` 保留 preview。宿主无法确认
  插入结果时必须返回错误，且不得自动重试。

`resolve_instruction`、`set_preview` 和 `replace_preview` 是 compatibility/headless 测试原语，不是新 UI
工作流入口。平台仍拥有麦克风、ASR native handle、窗口/热键、焦点恢复和 opaque insertion target；
correction、prompt、intent、EditPlan、translation 和 output-mode 判断不得进入 Tauri 或 egui。

### 3.5 设置事务

egui view model 必须从同一份快照取得偏好和 revision，再通过 `LinuxHost` 提交完整文档：

```rust
let snapshot = host.snapshot();
let outcome = host.update_settings_strict(
    preferences,
    snapshot.preferences_revision,
)?;
```

公开入口只有两种产品语义：

| 方法 | 冲突策略 | style 字段 | 适用场景 |
| --- | --- | --- | --- |
| `save_settings(preferences, revision)` | `Reconcile`：按 Core 固定优先级恢复旧值或停用低优先级键 | 保留当前值，防止陈旧整表覆盖刚发生的 style 切换 | 设置页整表保存 |
| `update_settings_strict(preferences, revision)` | `Reject`：任何冲突返回 `InvalidArgument` | 使用提交值 | 单项快捷键或明确的聚焦更新 |

Core 在单写入 gate 内完成 legacy 字段同步、style-pack 对齐、冲突校验/协调和 typed effect plan，
再按 `prepare -> commit effects -> persist once -> publish once` 执行。Linux Adapter 只消费计划中的
显式目标，不读取或修改 `UserPreferences`；失败时按 typed receipt 逆序恢复已应用的 fcitx5/凭据
副作用。成功只增加一次 `preferences_revision` 并发布一次 `PreferencesChanged`。

revision 不匹配时返回 `BackendErrorCode::Busy`、`retryable = true`，`details` 包含
`expectedPreferencesRevision` 和 `actualPreferencesRevision`。UI 必须重新读取 snapshot/偏好、合并
用户仍想保留的编辑后再提交；不得无条件重放陈旧整表。

Linux settings Adapter 当前支持 dictation、QA、Selection Polish、translation、Coding Agent 的 fcitx5 热键和
active ASR provider metadata；启动时同步保存的热键，禁用 Coding Agent 时解绑其语音键。修改 switch-style、open-app、style-pack hotkey 或
Windows keyboard effect 会稳定返回 `Unsupported`，且不写偏好、不增加 revision、不发布事件。
UI 不得直接调用 `OpenLessBackend::set_preferences*`；这些低层兼容方法不属于 Linux UI Interface，
也不能绕过 `LinuxHost` 的 revision、冲突和补偿契约。

## 4. DTO 字段契约

### 4.1 `DictationStateSnapshot`

| 字段 | 类型 | 规则 |
| --- | --- | --- |
| `phase` | `DictationPhase` | `idle/starting/recording/transcribing/polishing/inserting/completed/cancelled/failed` |
| `sessionId` | `SessionId?` | 非 `idle` 时存在；用于丢弃晚到结果 |
| `elapsedMs` | `u64` | 毫秒，不在 UI 侧换算成秒后再回写 |
| `level` | `f32` | 规范化到 `0..=1`；无音频时为 `0` |
| `message` | `String?` | 非敏感、用户可读提示；不可放 token/PIN |
| `translationActive` | `bool` | 会话开始时冻结；用于两个宿主显示当前会话的翻译状态 |

阶段是后端事实，不由 UI 猜测：

| phase | 含义 | egui 操作规则 |
| --- | --- | --- |
| `idle` | 无活动 session | 允许开始 |
| `starting` | engine/录音资源正在启动 | 显示准备态；允许取消，不允许重复开始/停止 |
| `recording` | 正在采集音频 | 允许停止或取消 |
| `transcribing` | ASR 正在处理/输出增量 | 禁用重复停止；可显示 `TranscriptDelta` |
| `polishing` | LLM/规则润色正在处理 | 可显示 `PolishDelta` |
| `inserting` | 已提交文字插入请求 | 不自动重试；等待明确/fallback/unknown 结果 |
| `completed` | 本 session 唯一成功终态 | 读取 `DictationResult` 后清理本地 session |
| `cancelled` | 用户、关闭或 generation guard 取消 | 丢弃该 session 后续增量 |
| `failed` | 本 session 失败 | 按 `BackendErrorCode` 提供重试或用户操作 |

### 4.2 `DictationResult`

`sessionId`、`rawText`、`polishedText` 和 `inserted` 均为 owned 字段。公开 Rust 类型
`DictationInsertStatus` 是 `InsertStatus` 的稳定契约别名；`inserted` 的 serde 值为
`inserted`、`copiedFallback` 或 `unknown`。`unknown` 表示插入请求超时后结果不可证明，
UI 必须显示待确认状态，不得自动重试以免重复输入。

### 4.3 能力与秘密

`PlatformCapabilities` 只描述能力布尔值和平台标识。`CredentialsStatus` 只包含
已配置的 provider id；凭据值、Authorization header、PIN 和完整请求永不进入
快照、事件、错误 details 或日志。

`PreferencesChanged` 事件只携带单调递增的 `revision`，UI 收到后重新调用设置
查询；不要把任意 JSON 或秘密塞进事件 payload。

所有 DTO 使用 Rust 的 `serde` 定义；与 React IPC 的兼容字段由 Tauri adapter
显式转换，不能让 egui 依赖 React 字段名。

### 4.4 `QaSnapshot` 与 Remote Input

`QaSnapshot` 是 QA view model 的唯一状态来源：

| 字段 | 类型 | 规则 |
| --- | --- | --- |
| `phase` | `QaPhase` | `idle/recording/thinking/awaiting_approval/completed/cancelled/failed` |
| `sessionId` | `SessionId?` | 每轮 turn 的 generation token；每个成功 follow-up turn 都分配新 ID，所有 progress、cancel 和迟到结果 guard 均绑定此 ID |
| `conversationId` | `SessionId?` | 同一面板成功多轮间稳定的 Selection Voice preview owner；仅在 dismiss/clear 后清空，不用于接受上一轮迟到结果 |
| `messages` | `QaMessage[]` | 有序 user/assistant message log；UI 不自行补写 provider 结果 |
| `editInstructionMode` | `bool` | 只能在非活动 turn 修改；活动 turn 修改返回 `Busy` |
| `pendingApprovalToken` | `String?` | 只用于显示和提交明确审批；不得当作跨 session 全局 token |
| `lastError` | `String?` | 已脱敏的公开错误；不得包含 provider body、Authorization 或选择全文 |

`QaLevel` 规范化为 `0..=1`；`AnswerDelta` 只对匹配当前 `sessionId` 且处于
`thinking/awaiting_approval` 的 turn 有效。成功 turn 保留 `conversationId`，`sessionId` 继续标识
该轮终态；下一轮开始时必须替换为新的 generation token，因此上一轮迟到 delta 不能污染 follow-up。
`dismiss()` 幂等地取消活动 runtime、清空 snapshot、清理与 `conversationId` 匹配的 Selection
Voice preview 并请求宿主隐藏面板；窗口 focus、NSPanel 和键盘仲裁不是 `QaSnapshot` 字段。

`LessComputerEvent` 是 Coding Agent 对话的唯一 UI 事件源：

| `kind` | 字段 | 规则 |
| --- | --- | --- |
| `voice_state` | `sessionId`, `phase`, `level`, `elapsedMs` | Core语音快照；phase为`starting/recording/transcribing/idle`，按原`seq`去重；旧session终态不得覆盖新录音 |
| `user` | `text`, `fresh` | Core 接受输入后发布；`fresh=true` 表示 dismiss 后的新会话 |
| `started` | — | runtime 已启动 |
| `delta` | `text` | 增量输出，按事件 `seq` 去重后追加 |
| `tool` | `name` | 仅展示工具活动；UI 不执行工具 |
| `compaction` | — | provider 正在压缩上下文，可作为非阻塞提示 |
| `approval` | `token`, `command`, `reason` | 只展示脱敏 command/reason，并把 token 原样回传 `approve`；不得写日志/持久化 |
| `completed` | `text`, `costUsd?` | 唯一成功终态 |
| `error` | `message` | 唯一失败终态，message 已脱敏 |
| `cancelled` | — | 唯一取消终态 |

Core 维护实例级 conversation flag、最多两轮 dsh continuation、approval token registry 和
90 秒 approval timeout。`dismiss()` 会取消当前 runtime、拒绝所有 pending approval、清空
continuation，并使下一轮 `user.fresh=true`。egui 只保存渲染所需的派生消息，不复制上述状态机。

语音显示还可从`backend.event_publisher().latest_less_computer_voice_state()`读取最后一条有效投影，保留原session/seq且占用固定一条空间。Tauri既有`less_computer_sync`返回可选`voiceState`，即使长转写的阶段事件已被2048条replay驱逐，重开也能恢复；该投影不推进聊天事件水位。合同版本仍为`2.0.0`，没有新增IPC入口。

`RemoteInputStatus` 字段和规则如下：

| 字段 | 类型 | 规则 |
| --- | --- | --- |
| `enabled` / `running` | `bool` | 前者是期望配置，后者是真实 transport 状态；两者不能互相替代 |
| `port` | `u16` | `1..=65535`；端口变化由 `configure` 串行 stop/restart |
| `urls` | `String[]` | 仅在 transport 成功绑定后存在 |
| `locale` | `String` | 仅接受 `zh-CN/zh-TW/en/ja/ko` |
| `connectionCount` | `usize` | 当前认证连接数，不包含已断开的历史连接 |
| `activeSessionId` | `SessionId?` | 任一连接正在推流时存在；仅作状态展示，不代替 connection/session 校验 |

pairing PIN 只能通过 `read_pairing_pin()` 的 `SecretValue` 显式读取；不得加入
`RemoteInputStatus`、事件、错误、`Debug` 或普通日志。PCM frame 必须为非空、偶数字节、最多
65536 bytes 的 signed Int16 little-endian；音频格式固定 16 kHz mono。每个连接最多一个活动
stream，重复 start 返回 `Busy` 且不得覆盖原 session lease。stop/cancel/disconnect 后的 frame
返回 `Cancelled`，宿主不得自动新建 session 重放。transport restart 必须先取消旧 stream、
使旧 connection/session lease 失效；旧 lease 再次 start/feed/stop 时稳定返回 `Cancelled`。

## 5. 事件契约

M0 的可机器读取基线位于
[`linux-egui-command-event-baseline.json`](./linux-egui-command-event-baseline.json)。该文件
记录从当前 Tauri handler、React `listen()` 和 Rust `emit*()` 调用点观察到的 command/event
名称；迁移期间新增或删除名称必须先更新该基线，再更新对应 adapter 和兼容测试。

`BackendEvent` 包含 `sequence: u64`、可选 `session_id` 和 `BackendEventKind`。
每个 backend 实例的 sequence 从 1 开始严格递增；事件总线是通知而不是状态
真相。backend 同时保留最近 2048 条实例级 backlog；`replay_events_after(sequence)` 返回
`events/oldestSequence/latestSequence/truncated`。`truncated=true` 表示调用方的游标早于当前
backlog，必须先重新读取 facade/领域 snapshot，再从最新 sequence 续订。

Less Computer 的 Tauri compatibility UI 使用同一 replay 水位：mount 时先安装实时 listener，
再以 `afterSequence` 拉取 replay；同步窗口内到达的事件先进入 pending 队列，随后按
`replay.events -> pending` 顺序合并。带 `seq` 的事件按最大已应用 sequence 去重，无 `seq` 的
legacy fallback 仍需保留。`truncated=true` 时旧派生时间线不再可信，调用方先清空本地时间线，
把水位重置为 `oldestSequence - 1`，从本次保留的 replay 重新构建，再接续 pending；最终水位
至少推进到 `latestSequence`。egui 不需要复用 React helper，但必须实现相同的顺序与去重语义。

| 事件 | UI 处理 |
| --- | --- |
| `BackendStarted` / `BackendStopping` | 更新宿主生命周期状态 |
| `DictationStateChanged` | 替换录音/处理/终态和 level |
| `TranscriptDelta` | 按 session 和 offset 合并原文增量 |
| `PolishDelta` | 按 session 和 offset 合并润色增量 |
| `DictationCompleted` | 展示结果并根据 `inserted` 显示确认/回退提示 |
| `InsertFallback` | 显示 clipboard/fallback 状态，不重试插入 |
| `PreferencesChanged` | 使设置缓存失效并重新读取 |
| `CredentialsChanged` | 更新 provider 配置状态，不显示秘密 |
| `HistoryChanged` / `VocabularyChanged` / `StylePacksChanged` | 使列表缓存失效 |
| `DownloadProgress` | 更新模型下载进度 |
| `PermissionChanged` / `HotkeyStatusChanged` | 更新能力和降级文案 |
| `Notification` | 放入非阻塞通知队列 |
| `CodingAgentTest` | 按 `session_id` 消费 started/delta/tool/completed/error/cancelled 流 |
| `LessComputerEvent` | 按 payload `seq` 构建会话时间线；fresh user 清空旧会话 |
| `LocalAsrPrepareProgress` / `LocalAsrDownloadProgress` | 按 runtime/model 更新本地模型准备与下载状态 |
| `LocalAsrEngineChanged` | 替换当前本地 ASR runtime 快照 |
| `MicrophoneDevicesChanged` | 使设备列表失效并重新调用 `PlatformApi::microphone_devices()` |
| `QaLevel` / `QaState` | 按 QA session 更新录音电平、阶段、增量和消息列表 |
| `RemoteInputStatusChanged` / `RemoteInputFailed` | 更新远程输入状态/错误；事件永不携带 pairing PIN |
| `VocabularySuggestionsChanged` | 替换当前待确认纠正建议 |

当前 29 个 `BackendEventKind` 已由机器基线和 serde fixture 校验。30 个旧 Tauri event 均已
分类；12 个原 `migrationRequired` 事件现已通过 typed core event 进入同一事件总线，再由
`tauri_events.rs` 映射为旧 React 名称。复杂领域后续迁移只能替换事件生产者的
Implementation，不能重新建立 host-only 状态流。

订阅可能返回 `EventRecvError::Lagged(n)`。收到该错误时，UI 必须丢弃本地增量，
重新读取 `snapshot()` 或对应领域查询，然后继续订阅；不能静默使用旧状态。
`Closed` 表示 backend 已被销毁。终态事件只发布一次，旧 session 的任何晚到
事件都必须被 session id guard 丢弃。

egui frame 不得 `.block_on()`。`EventSubscription` 同时提供异步 `recv()` 和非阻塞
`try_recv()`；frame 只应使用后者：

```text
后台订阅任务 -> 有界 UI channel -> frame 每次非阻塞 drain
                              -> 更新 view model
                              -> request_repaint()
```

最小的非阻塞循环如下（`Empty` 只表示本帧没有更多事件，`Lagged` 必须触发快照重同步）：

```rust
loop {
    match events.try_recv() {
        Ok(event) => view_model.apply(event),
        Err(EventRecvError::Empty) => break,
        Err(EventRecvError::Lagged(_)) => {
            view_model.replace_from_snapshot(backend.snapshot());
            break;
        }
        Err(EventRecvError::Closed) => view_model.mark_backend_closed(),
    }
}
```

`openless-linux-egui` 另提供 `drain_events()`，把上述循环收敛成
`EventDrainOutcome::{Idle, Lagged, Closed}`；它不引用 egui 类型，因此 UI 组可在
`eframe::App::update` 中直接调用，只有 `Lagged` 时才回读 `LinuxHost::snapshot()`。

## 6. 错误与取消

核心返回 `BackendError { code, message, retryable, details }`。UI 只判断 code，
不解析 message。

| code | 典型情况 | UI 行为 |
| --- | --- | --- |
| `invalid_argument` | 参数或 session id 不合法 | 修正输入；不重试原请求 |
| `invalid_state` | 未启动或无活动 session | 重新读取 snapshot |
| `busy` | 已有活动 session | 禁用重复开始 |
| `cancelled` | 用户取消、关闭或 session 过期 | 清理本地草稿 |
| `permission_denied` | 麦克风/辅助功能被拒绝 | 提供系统设置入口 |
| `unsupported` | fcitx5、托盘或平台能力不存在 | 显示降级，不假装成功 |
| `provider` | ASR/LLM 请求失败 | 依据 `retryable` 提供重试 |
| `persistence` | 读写数据失败 | 保留当前页面状态并提示 |
| `platform` | host adapter 失败 | 显示平台诊断 |
| `internal` | 未分类内部错误 | 显示通用错误并记录 request id（若有） |

取消必须绑定 session。取消与停止并发时，停止路径在调用 inserter 前再次检查
session/phase；已取消 session 不得产生插入副作用。shutdown 对活动 session 发送
`Cancelled` 状态，并等待 host 侧录音/热键任务退出。

Windows TSF Adapter 把失败分成两类：连接/准备阶段的 definite failure 可以按冻结的策略
尝试 SendInput/clipboard fallback；请求写入 pipe 后的超时、断连或无法判定的响应属于
outcome-unknown，返回明确的 `BackendError` 且不得再次插入。Core 的公开结果只包含
`Inserted`、`CopiedFallback` 或错误；这样即使 TSF 提交迟到，也不会与 fallback 形成重复文本。

`DictationEngine::finish` 返回 `Result<EngineResult, EngineFailure>`。`EngineFailure`
除 `BackendError` 外，还携带 `EngineFailureStage::{Transcribing, Polishing}`、可选原文、
录音时长、ASR/润色实测耗时和实际录音归档状态。facade 据此统一写失败历史：

- ASR 启动、录音停止或 ASR finalize 失败使用 `transcribeFailed`；
- ASR 返回空白文本使用 `emptyTranscript`；
- 禁止回退的润色失败使用 `polishFailed`，并保留已产生的原文；
- 插入失败使用 `insertFailed`，并保留原文、最终文本和 `polishSource`；
- 失败不增加 activity，Failed 事件仍携带原 session id，资源释放后同步 snapshot 回到 Idle。

## 7. Host ports

`BackendDependencies` 由宿主注入，核心不在方法内部创建系统对象：

- `TaskSpawner`：后台任务执行器；Linux 可使用 Tokio，测试可使用确定性执行器。
- `DictationEngine`：完整的 `start(session, progress) -> finish(session, progress) ->
  cancel(session)` 录音、ASR/润色生命周期；`EngineProgressSink` 使用
  `RecordingLevel { elapsed_ms, level }`、`EngineStage`、`TranscriptDelta` 和 `PolishDelta`
  回报进度；`cancel` 也会在 backend shutdown 时调用，不暴露窗口或 UI 类型。迟到进度若
  session 已失效会返回 `Cancelled`，adapter 必须停止发送。
- `AudioRecorder` / `ActiveRecording`：宿主采集设备音频并输出规范化的
  16 kHz / mono / signed Int16 little-endian PCM；`ActiveRecording::stop(self)` 消费句柄，
  保证 finish/cancel 竞争时最多释放一次。可恢复录音通过 `RecordingArchive` 精确表示，
  `is_available()` 报告真实状态，`read_pcm()` 支持冻结 provider 的 silent retry，`discard()`
  删除实际归档；`RecordingEvent::Fatal` 实时进入 Core controller，不得仅按 preferences 猜测
  `hasAudioRecording` 或等到 stop 才报错。
- `TranscriptionEngine` / `TranscriptionSession`：在录音前建立 ASR session，持续消费 PCM，
  `finish()` 返回最终原文，`cancel()` 终止 provider 请求。
- `TextPolisher`：接收最终原文并产生润色结果/可选增量；失败是否回退原文由
  `PolishFailurePolicy` 统一决定，宿主不得另写一套 fallback 判断。
- `TextInserter`：fcitx5、AX、TSF 或 clipboard fallback 的统一会话接口：
  `begin(session, context)` 在录音/ASR 启动前捕获 opaque target 并返回
  `TextInsertionSession::{supports_streaming,write,copy,finish,cancel}`。Core `ActiveTextInsertion` 独占 streamed prefix、
  Unicode tail、final divergence 与 clipboard fallback reconciliation；Host 报告已消费源 Unicode scalar 前缀长度，包含按约定吞掉的 CR，失败字符不计入。平台准备不能流式时返回 `supports_streaming=false`，仍可一次性落字。
  target 无法恢复时必须返回明确 copied/error，不能向当前焦点盲写；所有终态都要恢复输入法、
  剪贴板和平台资源。
- `HostActions`：`ShowDictationFeedback`、`HideDictationFeedback`、打开系统设置、
  外部 URL 和通知等语义动作；不传窗口 label。

此外已经存在：

- `CredentialStore`：status、显式 secret read/write/remove、provider channel metadata；
- `ResourceResolver`：只解析相对资源路径，拒绝绝对路径与 `..` traversal；
- `TaskSpawner`：由宿主注入 runtime，core 不创建窗口线程或专用全局 runtime；
- `BackendServices`：复杂领域 Adapter 集合，缺失时使用稳定的 unsupported 实现。
- `QaRuntimeAdapter`：捕获 selection host context、持有 recorder/ASR/LLM/Coding Agent 资源并执行
  `prepare_text/start_recording/finish_recording/answer/cancel`；Core 拥有 session、phase、messages
  和迟到结果 guard，Adapter 不得复制这些状态。Tauri 与 Linux production factory 均注入真实
  runtime；编辑/回答路由继续由 `QaService` 决定，Adapter 只执行上下文、录音和 provider effect。
- `RemoteInputRuntimeAdapter`：PIN secret persistence、TLS/socket/WSS/H5、local IP 与共享听写桥接；
  Core 拥有配置、连接/session 关联、PCM 校验和 transport 生命周期规则。
- `TranscriptionRouter` / `TextPolisherRouter` / `DictationEngineRouter`：分别按会话快照中的
  ASR、LLM 与 traditional/Omni 选择固定 Adapter；`provider_id` 是 channel/scoped credential
  标识，`provider_type` 才是协议路由 key，二者不能混用；ID、type、model 在 session 开始时
  一次冻结，运行中切换 active channel 或更新注册项只影响下一会话，缺失 provider 返回
  `Unsupported`。重复 session 必须原子返回 `Busy`，不得覆盖原 Adapter 或 cancellation route。
- `SharedCloudTranscriptionEngine` / `SharedCloudTextPolisher` /
  `SharedAuxiliaryTextPolisher` / `SharedOmniDictationEngine`：Core 生产 Implementation，负责
  credential account、默认 endpoint/model、协议选择、extra headers/temperature 校验、流式输出、
  取消和 session 占用；宿主注入 `CredentialStore`、`AudioRecorder` 和 `TaskSpawner`，UI 不接触
  这些细节。实时 ASR provider 的发送、接收和关闭任务必须使用该 `TaskSpawner`，core 不得创建
  私有 Tokio runtime。
  Omni 的 API key、endpoint、model、extra headers 和 temperature 必须使用
  `CredentialKey.providerId == DictationContext.omni.providerId` 读取；宿主不得在读取期间改写或借用
  active provider。活动 provider 切换只影响下一会话，任何公开错误都不得包含旧、新 provider secret。

`PipelineDictationEngine` 固定执行顺序为：启动 ASR session → 启动录音并推送 PCM/level →
停止录音 → finalize ASR → 按会话归档策略处理成功录音 → 发布最终原文 delta → 润色 →
发布最终润色 delta → 返回结果。ASR 失败和空转写保留可恢复录音；非空 ASR 成功且
`recordAudioForDebug == false` 时请求 Adapter 删除归档；删除失败时继续报告真实的
`hasAudioRecording == true`，不能产生“历史显示无录音但文件仍在”的假状态。
Facade 在 session capture 时冻结启用的 correction rules；Pipeline 在 ASR 成功后、任何
Less Computer/polisher 调用前先应用，并只在实际变化时把规则前文本写入 `asrTranscript`。
非流式最终文本在插入前按同一规则收口；已流式落字的路径不得事后改写 history 制造屏幕/记录
不一致。禁用或格式无效的规则不生效，读取失败只记录非敏感 warning，不丢整段听写。

History 的 provider 归因也由 facade 统一完成：traditional 流程从冻结 context 记录 ASR
channel/model，并仅在实际使用 LLM 时记录 LLM channel/model；multimodal 流程的 ASR 字段和
`asrMs` 为 `None`，LLM 字段记录冻结的 Omni channel/model，`polishMs` 保留 Omni 调用耗时。
成功和失败记录遵循同一规则，宿主不得自行重写归因。
这些细分 ports 只用于宿主组装和测试注入，不是 UI use-case；egui view model 只调用 facade。
热键状态通过 `PlatformApi` 查询；Host 只发送携带同一 `press_id` 和单调时间的
pressed/released/combined 边沿。Core `HotkeyInterpreter` 统一解释 Toggle/Hold/Auto、modifier
grace、250ms debounce 和 450ms terminal cooldown；组合键在 start await 前后均能取消同一代次。
Host 不得再保存 cooldown、began-session 或重复的 mode policy。

## 8. Linux 非 UI Adapter 契约

`openless-linux-egui` 已交付以下宿主能力，不包含 `eframe::App`：

- `LinuxCredentialStore`：secret value 只写 Linux Secret Service/keyring；
  `credential-metadata.json` 仅保存 channel、active provider 与已配置 key 标识，并原子替换；
  删除按namespace+channel清理全部secret，失败保留可重试元数据；读操作不暴露尚未提交索引的孤立secret。
  只有Host提供`BackendConfig.home_dir`时才尝试导入旧`com.openless.app` vault/分片与该目录下的旧JSON；Core解析旧格式，Host执行幂等写入，新配置优先，全部成功才写标记，保留旧来源。None配置不隐式访问系统旧凭据或真实HOME。
- `LinuxResourceLayout` / `LinuxResourceResolver`：分别定义 development、AppImage、deb、rpm
  的资源根与 fcitx5 插件相对路径；
- fcitx5 Adapter：availability、DBus commit、selection read、hotkey sync、clipboard fallback；
  普通听写使用 `CaptureDictationTarget(s: session) -> b`、`CommitDictationTarget(ss: session, text) -> b`、
  `CancelDictationTarget(s: session) -> b` 冻结并释放原输入上下文；新目标不能替换活动 session 的原目标。
  Selection 同时冻结原上下文及 surrounding text/cursor/anchor；取消选中、移动光标或修改文本后拒绝替换，全局 PRIMARY 不能证明原选区仍存在。
  `CommitText(s: text) -> b` 的 `true` 才表示文字已提交到输入上下文，`false` 表示当前没有
  可用焦点输入上下文（例如启动或无焦点/headless 场景）。Rust Adapter 必须把 `false` 转为
  明确的插入失败/平台错误并按策略决定 clipboard fallback，不能向 Core 或 UI 报告假成功；
  DBus 调用本身也不得因 no-context 让 fcitx5 进程崩溃。AppImage 可从版本化资源同步用户插件，
  deb/rpm 只验证系统安装，不能覆盖用户文件；
- `LinuxCapabilitySnapshot`：明确区分 X11、Wayland 与 headless，以及 tray、overlay、fcitx5、
  updater 和麦克风能力；未知权限返回 `Unknown`/`Unsupported`，不伪造 granted；
- `LinuxHostActions`：线程安全队列、非阻塞 drain 与可选 wake/repaint callback；
- `LinuxSettingsRuntime`：只执行 Core `SettingsEffectPlan` 的显式目标，通过 fcitx5 DBus 同步
  dictation/QA/Selection Polish/translation/Coding Agent，并通过 Linux credential metadata 同步 active ASR
  provider；不支持能力返回稳定 `Unsupported`，失败按 receipt 逆序恢复；
- `LinuxCpalRecorder`：选择偏好设备或默认输入设备，在专用线程持有 cpal stream，把常见
  sample format 下混、重采样和量化为 core PCM 契约，并报告 `0..=1` level；runtime fault
  实时进入 Core recording controller，不等待用户 stop。
- `Fcitx5HotkeyListener`：监听 dictation press/release/combined、QA、selection polish 和
  translation signals，提供非阻塞 `drain()`、`take_error()` 和可停止/join 生命周期；
  selection 信号调用共享 `SelectionApi`，空闲态先到达的 translation 信号会固定到下一次
  dictation press 的会话快照，活动 session 不会被中途改写。
- `SingleInstanceBroker`：私有 Unix socket + process lock；第二实例把 typed launch intent 转发
  给 primary 并等待 acknowledgement，primary 非阻塞 drain 后通过 `LinuxHost` 调用 core。
- `LinuxBackendBuilder::from_shared_providers(config)`：唯一生产 factory，组装 Core 共享云
  ASR/LLM/Omni/Auxiliary router、Marketplace、QA、Remote Input、Selection、打包 Qwen runtime、
  传统 `PipelineDictationEngine`、recorder、inserter、credentials、platform services、host actions
  与 settings runtime，返回不包含 egui 类型的
  `LinuxBackendRuntime`；`new(...)` 只用于测试/特殊宿主。
- `LinuxHost::download_marketplace_archive`：保存 Core 已校验归档；只接受绝对 filesystem path，
  不创建缺失父目录、不覆盖已有文件，失败时不遗留部分文件。

`linux-egui/src/main.rs` 已使用 `eframe::run_native` 接入 `LinuxHost`、Core event、fcitx5
hotkey 与 Single Instance Adapter；UI 不读取 Core 私有模块，也不复制业务规则。

## 9. 测试夹具

`openless_core::testing` 提供：

- `RecordingHostActions`：记录 host action 顺序；
- `FixtureAudioRecorder`：推送固定 PCM/level，并记录 stop 次数；
- `FixtureTranscriptionEngine` / `FixtureTextPolisher`：固定 ASR/润色结果、错误和取消行为；
- `FixtureDictationEngine::successful/failing`：在不测试细分 Pipeline 时提供固定结果；
- `FixtureTextInserter::with_outcome/failing`：覆盖 inserted、fallback、unknown 和失败，并通过
  `actions()` 暴露 prepare/insert/cancel 的 session-scoped 调用顺序。
- `FixtureSelectionRuntime`：记录 capture、preview、apply、revert、cancel；Linux production
  Adapter 以 fcitx5 ticket 实现可见 preview、confirm/cancel/revert 与 stale guard。
- `RecordingRemoteInputRuntime`：不绑定 socket 的内存 transport，记录 server/audio
  start/stop/cancel 次数和 PCM frame，用于验证单 connection 单 stream、restart 取消、stale
  lease 与 secret-surface 契约；它不代表生产宿主具备 WSS 能力。
- `LinuxCapabilityFixture::x11_full/wayland_degraded/headless`：覆盖 X11 完整能力、
  Wayland/fcitx5/托盘/权限降级以及无桌面会话；这些 fixture 只描述状态，不探测测试机。

egui view model 测试应只使用这些 fixture 和 `BackendSnapshot`/事件，不启动窗口、
麦克风、网络或真实凭据库。最低 contract test 集合：

1. 启动/关闭幂等和事件顺序；
2. 开始 → 处理 → 完成的主链路；
3. ASR 失败、插入失败、fallback、unknown；
4. 错误 session 取消不会改变活动 session；
5. 事件 lagged 后 snapshot resync；
6. 序列化 DTO 不包含秘密字段。

## 10. 能力降级矩阵

| 能力 | Linux 状态 | UI 规则 |
| --- | --- | --- |
| 全局热键 | available / unavailable | 不可用时隐藏快捷键设置或给出降级说明 |
| fcitx5 插入 | plugin missing / ready | missing 时允许 clipboard fallback，不能假成功 |
| 托盘 | available / unavailable | 不可用时保留主窗口内退出入口 |
| 悬浮反馈 | X11 / Wayland limitation | feedback 失败不升级为 ASR 失败 |
| 本地 ASR | model absent / ready | 显示下载、准备、释放状态 |
| 自动更新 | package-dependent | 不显示假更新按钮 |
| 麦克风 | granted / denied / no device | 区分权限拒绝和无设备 |

## 11. 版本与变更流程

当前代码常量为 `openless_core::BACKEND_CONTRACT_VERSION = "2.0.0"`。运行时 wire 只接受
2.0.0；1.x 兼容仅存在于 preferences、history、activity、credentials、model 和 style-pack
持久化迁移读取器中，不暴露 legacy runtime contract 常量。本文的 contract version 随破坏性
接口变更递增。新增可选 DTO 字段必须有默认
值；删除字段、改变枚举值、改变事件顺序或单位必须：

1. 更新 contract version；
2. 在计划文档的待决事项和迁移表记录影响；
3. 同时更新 Tauri mapping、Linux fixture 和示例；
4. 先让 contract tests 通过，再通知 egui 组切换。

egui 组发现缺少能力时，应提交一个只依赖 facade/DTO/event 的可复现测试；不得
读取 core 私有字段或复制内部实现。

### 11.1 从 0.1.0 迁移到 0.2.0

0.2.0 把可变偏好读取收敛为每会话 `Arc<DictationContext>`，属于有意的破坏性 Interface
变更：

- `DictationEngine::start` 新增 context；Pipeline 在 session 生命周期内持有同一快照；
- `AudioRecorder::start`、`TranscriptionEngine::start`、`TextPolisher::polish` 和
  `TextInserter::insert` 都接收同一 context；
- UI 不构造 context。宿主仍调用 `OpenLessBackend::start_dictation()`，或在需要翻译时调用
  `start_dictation_with_options(DictationStartOptions)`；facade 从 preferences、active style pack、
  provider metadata 与 vocabulary 一次性生成快照；
- 会话开始后修改麦克风、provider、模型、语言、风格包或插入策略只影响下一会话；
- 自定义 Adapter 必须停止在执行中重新读取 preferences，并只使用传入 context。

### 11.2 从 0.2.0 迁移到 1.0.0

1.0.0 扩展了听写结果和润色结果，用于让共享 core 独立持久化完整历史：

- `TextPolisher::polish` 的成功类型从 `String` 改为 `PolishOutput`；Adapter 应把最终文本放入
  `text`，组合润色加翻译时把润色后的源文放入 `source_text`，其他模式使用 `None`；
- `DictationEngine::finish` 的错误类型从 `BackendError` 改为 `EngineFailure`；Adapter 必须标注
  `EngineFailureStage`，并在已经产生时保留原文、录音时长、ASR/润色耗时和归档状态；
- `EngineResult` 新增 `polish_source`、`polish_failed`、`asr_ms`、`polish_ms` 和
  `has_audio_recording`；公开 `DictationResult` 新增 `polish_source` 与 `duration_ms`；
- `ActiveRecording::has_archived_recording` 被 `archive() -> Option<Arc<dyn RecordingArchive>>`
  取代，使 Pipeline 能在 stop 消费录音句柄后精确保留失败录音或删除成功录音；
- `DictationResult` 的两个新增 serde 字段均有兼容默认值，因此 0.2.0 JSON fixture 仍可读取：
  缺失 `polishSource` 时为 `None`，缺失 `durationMs` 时为 `0`；
- 宿主不得在 Tauri/egui Adapter 重复拆解组合翻译输出或重复写入成功 history；这些语义由
  core Pipeline 与 facade 统一负责。
- `TextInserter` 现在具有 `prepare/insert/cancel` 会话生命周期，且三个方法都接收或绑定
  `SessionId`；旧的只实现 `insert(context, text)` 的 Adapter 必须迁移。Core 在 engine 启动前
  调用 prepare，并在启动失败、处理失败、取消和 shutdown 路径调用幂等 cancel。
- `DictationInsertionContext` 额外冻结 `windows_sendinput_newline_mode` 与
  `android_insert_strategy`，平台 Adapter 不得在会话执行中重新读取偏好。

### 11.3 从 1.0.0 迁移到 2.0.0

2.0.0 将跨平台业务 Implementation 收口到 `openless-core`：

- 模型清单、Range 下载、断点索引、SHA-256、staging/ready sentinel 和旧目录迁移统一由
  `ModelStore` 提供；宿主只注入模型根目录、原生 runtime 和 typed progress sink；
- Coding Agent 由 Core `CodingAgentRunner` 统一构造四种 provider 的请求、解析 stream、过滤
  `session_id` 并产生唯一终态，宿主只实现进程创建、stdio、kill/wait 和临时文件；
- 文档窗口、最小差异/词汇学习和其它协议纯函数位于 Core，Tauri/Linux 仅保留 AX、窗口、
  输入法、socket、keyring 等 Adapter；
- 1.x preferences/history/activity、凭据元数据、旧模型根目录/mirror/sentinel 与 style-pack
  origin 字段继续按迁移规则读取，不因 contract 升级丢失。

## 12. 当前交付状态

| 交付物 | 状态 |
| --- | --- |
| `openless-core` package 和无 Tauri 依赖门禁 | 已建立 |
| facade 生命周期、听写状态机、事件 sequence | 已建立 |
| headless Linux host 示例 | 已完成，位于 `linux-egui/examples/headless_host.rs`；覆盖生命周期、数据领域、Less Computer、听写、Selection/Selection Voice、QA 与 Remote Input contract；真实 socket/窗口/设备证据另记 |
| fake host/recorder/transcription/polisher/engine/inserter/selection/remote transport | 已建立；fixture 固定完整业务状态，Linux production Adapter 另实现真实 TLS/WS、fcitx5 preview/revert 与 cpal effect |
| preferences/history/activity/vocabulary/correction/style-pack/credentials 共享实现 | 已建立；Tauri/Linux persistence Adapter 使用同一 Core mutation 与 active policy |
| Linux validated settings Interface | 已建立；`save_settings`/`update_settings_strict` 强制携带 snapshot revision，Core 统一校验、协调、持久化、事件和补偿，Linux Adapter 只消费显式 target |
| 全部复杂领域 DTO/Interface 与 unsupported 语义 | 已建立，位于 `domains.rs` / `BackendServices` |
| 2.0 公共 re-export 边界 | 已冻结；`openless-core`/`openless-linux-egui` 只公开 facade/DTO/event/host Interface/fixture，repository 与内部状态机不属于 UI 契约；`check-linux-public-surface.ps1` 防止边界回退 |
| Tauri command/event 完整迁移 | Core 业务路径已收口；React/CLI/Android JNI/Remote Input/桌面听写使用 2.0 contract，Tauri 仅保留 command/event wire 与平台 Adapter |
| 复杂领域真实共享 Adapter | Core `ModelStore`、`CodingAgentRunner`、Voice session、Provider policy、Remote Input 与 Style Pack 已接入；Linux Qwen runtime 已进入打包链，Foundry/Sherpa 与真实设备/发布物仍需平台证据 |
| 会话级 provider router | 已建立；ID/type/model 在 session 开始时固定，Core 持有云 ASR/LLM/Omni 协议 Implementation；Tauri 与 Linux 注册同一共享实现，Tauri 另行追加 native/local ASR |
| provider 验证/模型列表管理面 | 已建立；Core `ProviderService` 统一 channel-scoped credential、静态/远端模型列表、验证探活和错误脱敏；Tauri command 只做 wire 转换，Linux shared factory 注入同一 service；真实网络/keyring 和平台 runner 仍按主计划 M9/M10 留证 |
| Linux credentials/resources/fcitx5/capabilities/host-actions | 已建立非 UI Adapter 和 contract tests；WSL Ubuntu 已显式通过真实 Secret Service set/read/remove、fcitx5 plugin/method/listener/signal contract；无焦点输入时 plugin 不抛异常导致 fcitx5 崩溃 |
| Linux cpal 录音、共享 Pipeline builder、热键 listener、第二实例 intent 转发 | 已建立；selection/translation 已路由到共享 Interface；WSL 当前无 ALSA 设备时 cpal contract 已证明稳定分类错误，真实设备和桌面 runtime 生命周期仍见计划 M8/M9 |
| Linux 打包 workflow/manifest 契约 | 已建立但正式发布仍需真实 Ubuntu 安装、运行、升级和回滚证明 |
| egui UI | 已使用真实 `eframe::run_native`，覆盖 startup/error、听写、QA、Remote Input、Selection preview/revert、Less Computer/approval、Provider/Credential、模型、history 与 settings；视觉深化不属于 2.0 Core 收口 |

完整验收以主计划第 12 节为准；本契约证明 Linux UI 可以在不依赖 Tauri 的前提下使用冻结的
2.0.0 Interface。真实 Ubuntu 原生能力、发行包与安装升级回滚仍由 Linux runner 门禁证明。

## Less Computer 语音接口（2.0）

`OpenLessBackend::start_less_computer_voice(session_id, recording_control)` 返回 Core-owned
`LessComputerVoiceSession`。Host 必须只发送 16 kHz、mono、signed 16-bit little-endian
PCM；空帧、奇数长度和累计超过 provider 上限会返回 `InvalidArgument`。`finish` 只允许调用
一次，ASR 失败或空 transcript 不会启动 Agent，并释放 capture lease；`cancel` 同时取消
ASR/Agent 并释放尚未提升的 lease。`recording_control` 是窄平台 effect：Core 的共享
`SilenceAutoStop`/fault controller 决定 stop 或 cancel，Host 只关闭自己持有的 capture handle。

实时 provider 的 interim 文本通过既有 `BackendEventKind::TranscriptDelta` 发布，使用同一
`session_id` 且 `offset` 单调递增；批式 provider 只发布一次 `is_final=true`。Agent 阶段继续
使用 `LessComputerEvent`（approval、stream、completed、cancelled、error），不新增 ASR 事件
类型，Backend contract 版本为 `2.0.0`。

Linux `LinuxHotkeyEvent::{LessComputerPressed,LessComputerReleased,LessComputerCombined}`
只表达热键边沿；Hold/Toggle/Auto（Auto 长按阈值 350ms）由 Core 解释。三种录音入口与
`silence_auto_stop_enabled` 共用同一设置，冲突时保留当前会话并返回 `Busy`。
