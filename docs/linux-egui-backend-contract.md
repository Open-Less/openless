# Linux egui 后端接口契约（2.0.0）

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。范围以[2.0 需求](2.0-requirements.md)为准；本文是长接口与实现参考，交接材料以[交接目录](linux-egui-handoff/README.md)为准。

## 1. 合同文件

`openless-all/app/contract/backend-2.0.json`（`contractVersion` 2.0.0）顶层键：

| 键 | 内容 |
| --- | --- |
| `startupSnapshot` | 启动快照结构与版本校验规则；UI 必须先消费快照再渲染 |
| `backendEvent` | 语义事件清单、顺序与重放规则 |
| `lessComputerVoice` | Less Computer 语音事件面 |
| `androidJni` | Android JNI 合同（src-tauri android 桥接共用） |
| `linuxFacade` | Linux 专用 facade 面（`LinuxHost` 公开方法对应） |
| `enums` | 共享枚举（provider 类型、状态、错误等） |

## 2. Linux 侧公开签名（源码为准）

- `linux-egui/src/lib.rs`：`pub struct LinuxHost`；`LinuxHost::new(Arc<OpenLessBackend>)`、`with_settings_runtime`、`backend()`、`subscribe() -> EventSubscription`、`snapshot() -> BackendSnapshot`、`save_settings(...)`、`update_settings_strict(...)`、`drain_events(...)`、`feed_less_computer_pcm(&[u8])`。
- `linux-egui/src/backend.rs`：`LinuxBackendRuntime`；`LinuxBackendBuilder::from_shared_providers(BackendConfig)` + `with_task_spawner / with_recorder / with_auxiliary_polisher / with_text_inserter / with_credential_store / with_services / with_host_actions / with_settings_runtime / with_local_asr_runtime / with_polish_failure_policy` → `build() -> LinuxBackendRuntime`。
- 事件消费：`drain_events`（`lib.rs`）批量取走 Core 语义事件；订阅经 `EventSubscription`。

## 3. 注入接口（`crates/openless-core/src/ports.rs`）

`AudioRecorder`、`TextPolisher`、`TextInserter`、`CredentialStore`、`SettingsRuntime`、`TaskSpawner`、`EditObservationSink`/`EditObservationAdapter`（默认 `NoopEditObservationAdapter`）、`LinuxHostActions`。缺省实现表示"未接线"，不是"不支持"。

## 4. 与桌面共享

- 云 ASR/LLM/Omni/Auxiliary 实现两端共用（Core `asr/`、`provider_*`、`omni`、`llm_gemini`）；平台 Host 只注入原生录音、凭据、窗口、进程、焦点与插入 Adapter。
- 旧 React command/event 名称只保留在 Tauri 兼容 Adapter；Linux 与 Core 同进程，经类型化 Rust Interface 调用。
- provider 公开目录：Core `provider_rules::provider_descriptors` → 生成 `src/lib/ipc/provider-descriptors.generated.json`（`cargo run --locked -p openless-core --example export_provider_descriptors`）。

## 5. 行为约定（合同级）

- 原生预加载必须使用请求的 target/provider type；平台无法准备流式时以 `supports_streaming=false` 保留一次性落字。
- Remote stop 保持可取消的 session；socket 下行只转发本连接所属 session 的事件。
- 自动 contract 不替代真实平台证据：Ubuntu/Windows/macOS/Android 的设备、安装、升级与签名结果按[验收](linux-egui-handoff/07-acceptance.md)与[桌面验收](2.0-desktop-acceptance.md)分别记录。
