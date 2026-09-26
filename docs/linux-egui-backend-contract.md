# Linux egui 后端接口契约（2.0.0）

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-27。范围以[2.0 需求](2.0-requirements.md)为准；本文是长接口与实现参考，交接材料以[交接目录](linux-egui-handoff/README.md)为准。

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

- `linux-egui/src/lib.rs`：`pub struct LinuxHost`；`LinuxHost::new(Arc<OpenLessBackend>)`、`with_settings_runtime`、`backend()`、`subscribe() -> EventSubscription`、`snapshot() -> BackendSnapshot`、`save_settings(...)`、`update_settings_strict(...)`、`update_preference_fields(...)`（`preference_patch.rs`） 、`drain_events(...)`、`feed_less_computer_pcm(&[u8])`。
- `linux-egui/src/backend.rs`：`LinuxBackendRuntime`；`LinuxBackendBuilder::from_shared_providers(BackendConfig)` + `with_task_spawner / with_recorder / with_auxiliary_polisher / with_text_inserter / with_credential_store / with_services / with_settings_runtime / with_polish_failure_policy` → `build() -> LinuxBackendRuntime`。
- 事件消费：`drain_events`（`lib.rs`）批量取走 Core 语义事件；订阅经 `EventSubscription`。

## 3. 注入接口（`crates/openless-core/src/ports.rs`）

`AudioRecorder`、`TextPolisher`、`TextInserter`、`CredentialStore`、`SettingsRuntime`、`TaskSpawner`、`EditObservationSink`/`EditObservationAdapter`（默认 `NoopEditObservationAdapter`）、`LinuxHostActions`。缺省实现表示"未接线"，不是"不支持"。

## 4. 与桌面共享

- 云 ASR/LLM/Omni/Auxiliary 实现两端共用（Core `asr/`、`provider_*`、`omni`、`llm_gemini`）；平台 Host 只注入原生录音、凭据、窗口、进程、焦点与插入 Adapter。
- 旧 React command/event 名称只保留在 Tauri 兼容 Adapter；Linux 常驻宿主与 Core 同进程，经类型化 Rust Interface 调用；独立 UI 子进程经 Unix socket JSONL v2 通信，先 Hello/Ready 握手再收业务快照和热键。关闭 UI 不停止宿主或 Backend。
- provider 公开目录：Core `provider_rules::provider_descriptors` → 生成 `src/lib/ipc/provider-descriptors.generated.json`（`cargo run --locked -p openless-core --example export_provider_descriptors`）。

## 5. 行为约定（合同级）

- fcitx5 为生产启动硬依赖：持有单实例锁后检查插件和会话 D-Bus，按需刷新并在 15 秒就绪预算内等待，再启动监听器、注册必需热键、启动 Core 和 Remote Input。失败清理已启动资源并显示错误窗口；关闭后以非零状态退出，外层锁覆盖完整生命周期。
- 设置保存状态与动作处理位于 `linux_app/settings_save.rs`：按序列化字段生成补丁、串行提交、只重试明确 revision 冲突（最多三次），失败保留草稿并显式重试；Core 合同与持久化格式不变。
- 窗口生命周期与 client 位于 `linux_app/window.rs`，协议门禁位于 `ui/bridge.rs`。显式显示窗口请求创建窗口或发送 FocusMain；Wayland 是否激活由合成器决定。
- 历史缓存位于 `linux_app/history.rs`：初始加载、history revision 变化、事件重放截断或显式刷新触发单任务读取，历史文件和 WAV 探测只在 `spawn_blocking` 中进行；概览复用缓存。
- Linux 当前不包含任何本地 ASR 推理运行时；Qwen、MLX、Foundry 均明确不支持，不记作待设备验证。

- 原生预加载必须使用请求的 target/provider type；平台无法准备流式时以 `supports_streaming=false` 保留一次性落字。
- Remote stop 保持可取消的 session；socket 下行只转发本连接所属 session 的事件。
- 自动 contract 不替代真实平台证据：Ubuntu/Windows/macOS/Android 的设备、安装、升级与签名结果按[验收](linux-egui-handoff/07-acceptance.md)与[桌面验收](2.0-desktop-acceptance.md)分别记录。
