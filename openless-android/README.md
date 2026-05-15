# OpenLess Android

OpenLess 听写链路的 Android 原生重写版本。

桌面版 OpenLess 依赖全局热键与桌面插入 API。Android 不提供这类原语，因此本迁移采用 Android 等价机制：

- 用可拖动悬浮触发器替代全局热键
- 用麦克风前台服务替代后台录音
- 用可选 OpenLess 输入法完成直接插入
- 当 IME 不可用时，用剪贴板兜底
- 在支持的应用中，用 Android `PROCESS_TEXT` 完成“选中文本 -> 问答”跳转

## 当前功能

- 火山 SAUC 流式 ASR 听写
- Whisper 兼容 `/audio/transcriptions` 兜底
- OpenAI 兼容润色链路
- `原文 / 轻润色 / 结构化 / 正式` 四种模式
- 可配置目标语言的一次性翻译流程
- 词典热词同时注入 ASR 与润色提示词
- 当 IME 上下文可用时，历史记录写入目标应用元数据
- 提供商诊断：LLM、ASR 配置检查、模型列表
- Android 诊断：麦克风、悬浮窗、通知、前台服务、IME 状态
- 问答面板：文字提问、语音提问、剪贴板上下文、历史上下文、流式回答
- 通过 Android 文本选中动作，把选中文本送入问答

## 构建

当前模块不依赖 Gradle，直接使用本地 Android SDK 工具构建：

```powershell
cd openless-android
.\build.ps1
```

调试 APK 输出：

```text
openless-android\build\OpenLessAndroid-debug.apk
```

Android 版本元数据与桌面源同步，来源于：

- `openless-all/app/package.json`
- `openless-all/app/src-tauri/tauri.conf.json`
- `openless-all/app/src-tauri/Cargo.toml`

构建会校验这三处版本一致，并将其作为 `versionName`，同时按 `major * 10000 + minor * 100 + patch` 推导 `versionCode`。

## 配置

打开应用后，可在“设置”中填写：

- ASR provider：`volcengine` 或 `whisper`
- 火山 ASR 应用 Key / 访问 Key / 资源 ID
- Whisper 兼容 ASR 服务地址 / API Key / 模型
- LLM 服务地址 / API Key / 模型
- 启用的润色模式
- 工作语言
- 翻译目标语言
- 悬浮胶囊显示开关
- 剪贴板兜底开关
- 问答历史保存开关

主界面诊断入口：

- `检测 LLM`
- `检测 ASR`
- `列出 LLM 模型`

词典支持：

- 启用 / 停用
- 删除
- 备注
- 命中计数

启用的词条会同时进入润色提示词与火山热词上下文。

## 主要流程

### 听写

1. 授予悬浮窗权限
2. 启动悬浮触发器
3. 点击气泡开始录音
4. 再点一次结束
5. OpenLess 完成转写，必要时润色/翻译，然后通过 IME 插入或回退到剪贴板

通知动作：

- `剪贴板问答`
- `问答面板`
- `翻译`
- `取消`
- `停止`

### 直接插入

1. 点击 `启用键盘`
2. 在 Android 输入法设置中启用 OpenLess
3. 在目标输入框切换到 OpenLess 输入法
4. 使用听写

如果 OpenLess IME 正在激活，文本会直接提交到当前输入框；否则只有在启用剪贴板兜底时才会复制结果。

### 问答

可从以下入口进入：

- `打开问答面板`
- `剪贴板问答`
- 历史记录中的“问答”
- 悬浮通知中的 `问答面板` / `剪贴板问答`
- 在支持的应用里选中文本，再选择 `OpenLess`

问答支持：

- 粘贴或选中文本上下文
- 内存中的多轮会话
- 面板内语音提问
- OpenAI 兼容流式回答

## 桌面到 Android 映射

| 桌面 OpenLess | Android 重写 |
| --- | --- |
| Tauri/Rust coordinator | `AndroidDictationCoordinator` |
| 全局热键 | 可拖动悬浮气泡 |
| Recorder | `AudioRecorder`，16 kHz 单声道 PCM |
| 火山流式 ASR | `VolcengineStreamingSession` + `VolcengineFrameCodec` + `SimpleWebSocket` |
| Whisper 批量 ASR | `WhisperAsrProvider` |
| LLM 润色与问答 | `OpenAiPolishProvider` + `OpenLessPrompts` |
| Capsule 状态事件 | `FloatingTriggerService` + `CapsuleState` |
| 插入层 | `TextInserter` + `OpenLessInputMethodService` |
| 历史存储 | `HistoryStore` |
| 词典 | `DictionaryStore` |
| 安全凭据 | `SecureValueStore` |

## 状态文档

参见：

- `PORT_STATUS.md`
- `QA_CHECKLIST.md`
- `RELEASE.md`
