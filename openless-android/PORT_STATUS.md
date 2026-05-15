# OpenLess Android 迁移状态

本模块是 OpenLess 桌面端听写链路的 Android 原生重写版本。它不是逐文件照抄，而是把协议形状、状态流转、提示词行为、持久化结构迁移到 Android Java 实现中。

实施计划：

- `../docs/plans/2026-05-04-openless-android-full-port.md`
- `../docs/plans/2026-05-05-openless-android-ui-rebuild-execution.md`

## 源码映射

| 原始源码 | Android 端 | 状态 |
| --- | --- | --- |
| `openless-all/app/src-tauri/src/coordinator.rs` | `src/com/openless/android/AndroidDictationCoordinator.java` | 已迁移核心状态流：idle / start / listen / process / cancel |
| `openless-all/app/src-tauri/src/asr/frame.rs` | `src/com/openless/android/VolcengineFrameCodec.java` | 已迁移二进制帧编解码 |
| `openless-all/app/src-tauri/src/asr/volcengine.rs` | `VolcengineStreamingSession.java`, `VolcengineAsrProvider.java`, `SimpleWebSocket.java` | 已迁移流式 SAUC 生命周期 |
| `openless-all/app/src-tauri/src/asr/whisper.rs` | `WhisperAsrProvider.java`, `WavEncoder.java` | 已迁移 OpenAI 兼容转写路径 |
| `openless-all/app/src-tauri/src/polish.rs` | `OpenAiPolishProvider.java`, `OpenLessHttp.java` | 已迁移 OpenAI 兼容润色/问答请求 |
| `openless-all/app/src-tauri/src/types.rs` `PolishMode` | `PolishMode.java` | 已迁移 `raw/light/structured/formal` |
| `openless-all/app/src-tauri/src/types.rs` `DictationSession` | `HistoryStore.java` | 已迁移历史结构到 Android SharedPreferences JSON |
| 桌面词典持久化 | `DictionaryStore.java` | 已迁移词条结构、热词启停、命中计数 |
| 桌面凭据存储 | `SecureValueStore.java` | 已映射到 Android Keystore AES-GCM |
| 桌面热键 + capsule | `FloatingTriggerService.java` | 已映射为 Android 悬浮窗前台服务 |
| 桌面插入层 | `OpenLessInputMethodService.java`, `TextInserter.java` | 已映射为 Android IME 直接插入 + 剪贴板兜底 |

## 已完成

- 悬浮触发器已替代桌面端全局热键
- 支持单击开始/结束听写、拖动定位、长按取消
- Android 14 兼容的麦克风前台服务
- 16 kHz 单声道实时录音
- 火山 SAUC 流式 ASR
- Whisper 兼容批量 ASR
- OpenAI 兼容润色链路
- `原文 / 轻润色 / 结构化 / 正式` 四种模式
- 词典热词同时注入 ASR 上下文与润色提示词
- 词条支持：`id / phrase / note / enabled / hits / createdAt`
- 历史记录支持清空、单条删除、点击复制、长按问答
- 设置项已覆盖：
  - ASR / LLM 配置
  - LLM 提供商预设（Ark / DeepSeek / SiliconFlow / OpenAI / 自定义）
  - 工作语言
  - 翻译目标语言
  - 悬浮胶囊开关
  - 剪贴板兜底开关
  - 问答历史开关
- 设置页已改为结构化控件：
  - ASR provider 单选
  - LLM provider 单选预设
  - 模式多选
  - 布尔值开关
- 主界面已具备：
  - Android 诊断
  - 提供商诊断
  - 历史记录
  - 词典入口
  - 问答入口
  - 翻译一次
- `显示悬浮胶囊` 设置已接入实际悬浮服务行为：
  - 关闭后可隐藏悬浮气泡
  - 仍可通过常驻通知执行开始/停止听写、翻译、问答
- 常驻通知已按状态动态裁剪 action：
  - 空闲态显示 3 个核心入口
  - 录音/处理中切换为取消、问答、停止
  - 避免超过 Android 通知 action 的稳定显示上限
- 翻译已接入实际链路，并写入历史元数据
- OpenLess IME 激活时，可捕获目标包名并写入历史
- 问答已支持：
  - 粘贴上下文
  - 多轮会话
  - 流式回答
  - 语音提问
  - 历史转问答
  - Android `PROCESS_TEXT`
- 已吸收一轮新的 UI 视觉改造：
  - `QaPanelActivity` 的卡片/气泡/间距样式已合入
  - `FloatingTriggerService` 的悬浮气泡绘制已合入
  - `MainActivity` 已手工吸收一部分安全的视觉细化（历史列表、权限诊断、按钮尺寸）
- 主界面已完成第一轮信息架构重组：
  - 顶部增加分区导航：听写 / 历史 / 工具
  - 听写页新增概览卡片，集中展示 ASR / LLM / 模式 / 历史 / 翻译目标
  - 词典 / 问答快捷入口已收拢到工具页，减少主听写页按钮堆叠
- 设置已完成第一轮页面化迁移：
  - 新增 `SettingsActivity`
  - 主界面设置入口不再依赖长 `AlertDialog`
  - provider / 语言 / 听写 / 问答配置已按分区重组
- 页面化重构继续推进：
  - `QaPanelActivity` 已加入会话概览与设置/词典快捷入口
  - `DictionaryActivity` 已新增并接管主界面词典入口
  - 词典支持页面内新增、启停、删除、复制导出、剪贴板覆盖导入、清空
  - `SettingsActivity` 已支持按分区深链打开，并补充相关工具入口
  - `ModelListActivity` 已新增并接管模型列表展示，不再通过模型弹窗查看
  - `HistoryDetailActivity` 已新增并接管历史详情展示，不再通过详情弹窗查看
- Manifest 对外标签已资源化
- 版本号已与桌面源同步，并在以下文件间做一致性校验：
  - `package.json`
  - `tauri.conf.json`
  - `Cargo.toml`
- `verify.ps1` 已覆盖 APK 基本校验
- `verify.ps1` 现已支持自动发现 `adb` 并可通过 `-CheckDevice` 报告设备连接状态
- `deploy.ps1` 已新增，可在有设备连接时执行 APK 安装与可选启动
- `QA_CHECKLIST.md`、`RELEASE.md`、`STORE_SUBMISSION_CHECKLIST.md` 已建立并中文化
- 历史项收尾迁移继续推进：
  - 长按历史项不再弹系统菜单，统一进入 `HistoryDetailActivity`
  - `HistoryDetailActivity` 已补删除动作
- 错误展示收尾迁移继续推进：
  - 新增 `ErrorDetailActivity`
  - `MainActivity` / `QaPanelActivity` 的错误不再走 `AlertDialog`
- 页面反馈层收口继续推进：
  - `MainActivity` 多处反馈已统一收敛到页内状态行
  - `QaPanelActivity` 的未输入、语音空识别、复制回答/对话等反馈已改为页内状态
  - `DictionaryActivity` / `HistoryDetailActivity` / `ModelListActivity` / `ErrorDetailActivity` 已补齐页内状态行
  - `SettingsActivity` 已拆分为页面状态与诊断状态，不再依赖 `Toast` 反馈保存/诊断结果
- 已完成一轮模拟器页面验收：
  - 已处理麦克风权限与通知权限弹窗
  - `MainActivity` 听写分区可正常显示
  - `MainActivity` 历史分区可正常显示筛选与空状态
  - `MainActivity` 工具分区可正常显示诊断卡与快捷工具卡
  - `SettingsActivity` 已修复启动时 `NullPointerException`，当前可正常打开
  - `DictionaryActivity` 可正常打开
  - `QaPanelActivity` 可正常打开

## 当前差距

- 尚未完成带真实凭据的完整听写/翻译/问答端到端录音验证
- 已确认本机可用 `adb` 路径：`C:\Users\16014\AppData\Local\Android\Sdk\platform-tools\adb.exe`
- 已确认可用模拟器：`emulator-5554`
- 发布签名、商店元数据、最终图标素材、商店截图仍未完全收口
- Android UI 已可用，但仍不是桌面 React UI 的完整视觉复刻
- 主界面和设置页已明显接近工具化产品，但尚未完整复刻桌面端完整设置/导航面
- 剩余 `Toast` 已只存在于 `FloatingTriggerService` / `AndroidDictationCoordinator` 的服务态无页面宿主链路
- 选中文本问答依赖 Android `PROCESS_TEXT`，无法像桌面端那样做完全通用的跨应用选区捕获
- Android 对跨应用上下文访问有限；非 IME 路径下的目标 app 元数据仍不稳定
- 提供商诊断目前以配置有效性检查为主，还不是完整实录音 round-trip 验证
- 火山流式链路仍需要带真实凭据的真机实测

## 当前构建

构建命令：

```powershell
.\build.ps1
```

输出：

```text
build\OpenLessAndroid-debug.apk
```

当前最新本地构建结果：

- 可成功编译
- 2026-05-05 本轮在以下改动后再次通过：
  - `MainActivity` 分区骨架
  - `SettingsActivity` 新增与 Manifest 注册
  - 听写页概览卡片与工具入口整理
  - `SettingsActivity` 初始化空指针修复
  - `ErrorDetailActivity` 新增与错误页接管
  - `HistoryDetailActivity` 删除动作补齐
  - 页面反馈层收口：设置页/错误页去 `Toast` 化
- 通过 `apksigner verify`
- `aapt dump badging` 可确认：
  - package：`com.openless.android`
  - version：与桌面源同步
  - launchable activity：`com.openless.android.MainActivity`
  - IME 组件存在
  - 麦克风 / 悬浮窗 / 前台服务 / 通知等权限已声明
