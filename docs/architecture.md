# OpenLess 架构与接口调用

OpenLess 由界面、宿主和共享 Core 三层组成。界面接收输入并展示状态。宿主连接各个平台的窗口、热键、麦克风、编辑器和凭据库。Core 在宿主进程内管理会话、业务规则、服务调用和数据。本文路径相对于仓库根目录。

## 模块职责

| 模块 | 职责与源码入口 |
| --- | --- |
| React 界面 | [src/app](../openless-all/app/src/app/) 负责启动与窗口路由，[src/features](../openless-all/app/src/features/) 负责业务页面，[src/data](../openless-all/app/src/data/) 管理快照、失效和写入，[src/api](../openless-all/app/src/api/) 提供分域调用。 |
| 宿主传输 | [src/runtime/transport.ts](../openless-all/app/src/runtime/transport.ts) 验证启动快照并发送进程间调用（IPC）。[src/preview](../openless-all/app/src/preview/) 实现浏览器预览，提供夹具数据及模拟事件。 |
| 共享 Core | [api.rs](../openless-all/app/crates/openless-core/src/api.rs) 定义 `OpenLessBackend` 和业务入口，[ports.rs](../openless-all/app/crates/openless-core/src/ports.rs) 定义麦克风、插入等宿主接口，[events.rs](../openless-all/app/crates/openless-core/src/events.rs) 定义语义事件。 |
| macOS、Windows 与 Android 宿主 | [src-tauri/src/lib.rs](../openless-all/app/src-tauri/src/lib.rs) 注册 IPC，[coordinator.rs](../openless-all/app/src-tauri/src/coordinator.rs) 组装 Core 和会话，[core_adapters.rs](../openless-all/app/src-tauri/src/core_adapters.rs) 对接宿主能力，[tauri_events.rs](../openless-all/app/src-tauri/src/tauri_events.rs) 将 Core 事件交给界面。 |
| Linux | [linux-egui](../openless-all/app/linux-egui/) 使用 egui 界面和常驻宿主。`LinuxHost` 直接调用 Rust 类型化接口，宿主与界面子进程经 Unix socket 交换请求和快照。见 [Linux 接口](development/linux.md)。 |
| HarmonyOS | [harmony](../openless-all/app/harmony/) 的 ArkTS 管理系统和输入法，ArkWeb 复用 React 界面，`harmony/rust` 经 Node-API 连接 Core。 |
| iOS | [ios](../openless-all/ios/) 的 Swift 应用、键盘扩展和录制小组件分别运行。`CoreHost` 是原生 Rust 桥，应用负责录音和 Keychain，扩展经 App Group 交换有限的命令与结果。 |

[backend-2.0.json](../openless-all/app/contract/backend-2.0.json) 记录启动快照、事件、枚举和宿主接口。改动跨层命令时同时核对 Rust、TypeScript、宿主注册和事件接收端。

## 请求怎样经过各层

~~~mermaid
flowchart LR
  UI["React 界面"] --> API["分域 API / 宿主传输"]
  API --> Host["Tauri commands / HarmonyOS bridge"]
  Native["Linux 或 iOS 原生界面"] --> Adapter["各自的宿主接口"]
  Host --> Core["OpenLessBackend"]
  Adapter --> Core
  Core --> Store["本地仓储"]
  Core --> Providers["服务商接口"]
  Core --> Ports["宿主接口：录音、凭据、落字"]
  Core --> Events["BackendEvent"]
  Events --> Host
  Events --> Adapter
~~~

### 启动与设置

1. [bootstrap.tsx](../openless-all/app/src/app/bootstrap.tsx) 选择窗口入口。传输层等待宿主就绪并读取 `get_startup_snapshot`，验证接口版本后再初始化业务界面。
2. React 通过分域 API 读取快照并订阅事件。设置保存由 `update_setting_fields` 提交字段差异，Core 按修订号持久化。`prefs:changed` 使界面重新读取；写入失败只撤回对应的未确认编辑。
3. 同一窗口内的快照查询由 `src/data` 处理重叠请求和失效。历史、活动统计、服务用量分开保存，保留和删除规则各自生效。

### 听写

热键或原生按钮 → 宿主录音会话 → [dictation_engine.rs](../openless-all/app/crates/openless-core/src/dictation_engine.rs) → 转写及可选润色 → 宿主落字 → 历史与终态事件。宿主提供麦克风、目标编辑器和实际写入结果。录音取消、错误和超时沿原会话清理资源，迟到结果按会话身份丢弃。

云识别和润色由 Core 选择渠道并发送请求。本地模型的下载和激活按模型家族由 Core 与宿主共同负责。点击胶囊、流式原文与写入目标编辑器分别走各自的界面和宿主路径。见[语音链路](development/speech.md)、[本地模型](development/local-models.md)和[自定义润色策略](development/polish-strategy.md)。

### 问答与选区

桌面问答窗口由 [windowRoutes.tsx](../openless-all/app/src/app/windowRoutes.tsx) 分派。界面发送 `qa_submit_text` 等命令给 [Tauri 问答适配](../openless-all/app/src-tauri/src/commands/qa.rs)，由 [Core 问答服务](../openless-all/app/crates/openless-core/src/qa_service.rs) 管理会话、请求和结果事件。[选区语音服务](../openless-all/app/crates/openless-core/src/selection_voice_service.rs) 根据语音意图生成编辑预览或问答，确认写回时由宿主复核原输入目标。Linux 的问答界面经 Linux 宿主调用同一业务层。

Android 浮窗从听写转到追问时，宿主先保存选区，再用 `stop_dictation_for_qa` 结束原录音取得转写，由 `QaApi::submit_captured_text` 提交问题。这个路径跳过原听写的润色和落字。

### 移动端输入法

Android 的 [OpenLessImeService](../openless-all/app/android/kotlin/OpenLessImeService.kt) 持有当前编辑器的 `InputConnection` 并提交文字。[native_bridge.rs](../openless-all/app/src-tauri/src/android/native_bridge.rs) 通过 JNI 调用 Rust；Runtime Service 与 Warmup Activity 负责后台服务和 WebView 的启动或恢复。HarmonyOS 由 ArkTS 输入法扩展调用系统 `InputClient`。两端共用 [Rime 引擎](../openless-all/app/mobile-ime/README.zh.md)，负责拼音候选与词库。切换编辑器使旧候选和异步识别结果失效，只有宿主确认文字已提交后才向引擎确认候选。

语音与词汇规则走 Core。密码框、禁止学习的输入框和敏感应用受宿主隐私策略约束；手改学词的观察和建议由 [Core 观察模块](../openless-all/app/crates/openless-core/src/host_document/) 管理，确认后才写入词典。Android 跨应用写入的无障碍、Shizuku 和剪贴板路径各自返回结果。

## 按功能定位代码

| 功能 | 改动入口 |
| --- | --- |
| 热键、权限、窗口、胶囊、目标插入 | [Tauri coordinator](../openless-all/app/src-tauri/src/coordinator/) · [界面窗口路由](../openless-all/app/src/app/windowRoutes.tsx) · [Windows TSF/IME](../openless-all/app/windows-ime/) |
| 服务渠道、凭据、语音识别与润色 | [Core 模块声明](../openless-all/app/crates/openless-core/src/lib.rs) 中的 `provider_*`、`asr`、`polish`、`omni`、`credentials`。服务密钥通过宿主系统库保存。 |
| 历史、词汇、纠错、风格和市场 | [Core 源码](../openless-all/app/crates/openless-core/src/) 中的 `history`、`vocabulary`、`correction`、`style_pack_store`、`marketplace`，配合对应的 `src/features` 页面。 |
| 加密云同步 | [Tauri 云同步命令](../openless-all/app/src-tauri/src/commands/cloud_sync_e2ee.rs) → Core `cloud_sync_e2ee` → protocol、documents 和 store 模块。加密、身份、合并和恢复在原生层完成。见[数据与协议](development/cloud-sync.md)。 |
| 远程输入 | [Tauri remote_server](../openless-all/app/src-tauri/src/remote_server/) 或 Linux 宿主 → Core 远程输入服务 → 外部音频会话。见[远程输入协议](development/remote-input.md)。 |
| UI、主题、语言、浏览器夹具 | [src/ui](../openless-all/app/src/ui/) · [src/styles](../openless-all/app/src/styles/) · [src/i18n](../openless-all/app/src/i18n/) · [src/preview](../openless-all/app/src/preview/) |

## 构建和验证从哪开始

[openless-all/app](../openless-all/app/) 是 npm、共享 Core 与 Linux 的 Cargo 工作目录。`src-tauri` 与 `src-tauri/backend-tests` 使用各自的 Rust 清单；原生 iOS 工程有独立版本和签名。依赖、工具链与命令见[全平台构建](../openless-all/README.zh.md)。

- 界面与接口变动从应用目录运行 `npm test`。修改生成的服务目录时，从 Core 运行 `cargo run --locked -p openless-core --example export_provider_descriptors`。
- Core 和 Linux 分别运行 `cargo test --locked -p openless-core` 和 `cargo test --locked -p openless-linux-egui`。Tauri 与所涉及平台的检查由 [CI 工作流](../.github/workflows/ci.yml) 定义。
- 自动检查、已安装应用与设备交互分别取证。打包和发布门槛见[发布规范](../RELEASING.zh.md)，仍缺的实现与验证见[开发需求](requirements.md)。
