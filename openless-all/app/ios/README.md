# OpenLess iOS · Swift 原生版

独立的 SwiftUI 应用和 UIKit 键盘扩展，面向 iPhone / iPad，最低 iOS 17。工程入口为 [OpenLess.xcodeproj](OpenLess.xcodeproj)，共享 Scheme 为 `OpenLess`。

这份实现只使用 Apple 系统框架，没有 Swift Package、CocoaPods、npm、Rust 或服务端部署依赖。iOS 业务由 Swift 实现，沿用 OpenLess 的四种风格与“整理文字而不回答问题”的产品语义；没有通过 FFI 链接 `openless-core`。

当前交付为源码与 Xcode 工程：按本次要求，没有安装开发环境，没有执行编译、测试、静态检查或界面验证，也没有生成 IPA。

## 已实现的流程

| 功能 | 实现 |
| --- | --- |
| 原生听写 | 麦克风权限、Apple Speech 权限、实时转写与音量显示；默认只允许设备端识别 |
| 兼容 ASR | 录制 AAC / M4A，通过 `audio/transcriptions` 上传至用户配置的 HTTPS 服务 |
| 文字整理 | 原文、轻度润色、AI 提示词、正式表达、自定义系统提示词 |
| 翻译 | 按设置的目标语言，调用用户配置的 `chat/completions` 服务 |
| 个人词典 | 新建、修改、删除和搜索；用于 Apple contextual strings、云端转写提示与润色上下文 |
| 历史与草稿 | 本地保存原文、结果、风格、时间与异常说明；搜索、删除、继续编辑与润色 |
| 结果交付 | 复制、系统分享、主动发送至 OpenLess 键盘；键盘插入当前文本框 |
| 凭据 | 两类 API Key 分别保存于系统钥匙串，不进入 JSON 或 App Group |
| 中断处理 | 电话打断、耳机断开、时长上限、转后台结束录音；转写失败后保留云端录音供重试 |
| 界面 | 简体中文、系统 / 浅色 / 深色外观，iPhone 与 iPad 自适应宽度，基础 VoiceOver 标签 |

首次启动默认选择 **Apple 语音识别 + 原文 + 关闭翻译**。设备和语言支持离线识别时，无需配置云端密钥即可开始听写。若设备端识别不可用，应用显示原因，不会自动改为上传音频。

## 工程结构

```text
ios/
├── OpenLess.xcodeproj/               # 两个 target、共享 Scheme、内嵌键盘扩展
├── Configuration/Project.xcconfig    # 统一 Bundle ID、App Group、签名与最低系统版本
├── OpenLess/
│   ├── App/                         # SwiftUI 入口、状态、持久化和听写任务编排
│   ├── Services/                    # 音频、HTTP、钥匙串、本地 JSON
│   ├── Views/                       # 听写、历史、词典、风格、设置和键盘指引
│   ├── Resources/                   # 复用仓库图标、隐私清单
│   ├── Info.plist
│   └── OpenLess.entitlements
├── Keyboard/                        # UIKit 键盘、独立 Info.plist / entitlements / 隐私清单
└── Shared/                          # Codable 数据结构及键盘共享文件协议
```

## 在已有 Mac 开发环境中打开

使用已有 Xcode 16 或更新版本打开 `OpenLess.xcodeproj`，选择 `OpenLess` Scheme。工程不需要先执行生成脚本。

签名信息集中在 [Configuration/Project.xcconfig](Configuration/Project.xcconfig)：

- `DEVELOPMENT_TEAM`：填写自己的 Apple 开发团队 ID。
- `OPENLESS_BUNDLE_ID`：默认 `com.openless.ios`，按开发账号注册情况修改。
- `OPENLESS_APP_GROUP`：默认 `group.com.openless.ios`，在自己的团队注册并授权给两个 target。

主应用 Bundle ID 使用 `OPENLESS_BUNDLE_ID`，扩展使用 `$(OPENLESS_BUNDLE_ID).keyboard`。两个 target 的 entitlements 和 `Info.plist` 都引用同一个 `OPENLESS_APP_GROUP`；不要分别填入不同的组名。仓库不包含任何签名证书或描述文件。

App Group 尚未配置时，主应用的听写、历史、复制和分享仍可使用，但发送至键盘会提示共享空间不可用。源工程独立于 Tauri 的 `src-tauri/gen/apple`，不要使用 `tauri ios init` 覆盖此目录。

## 模型配置

在设置页填写服务地址与模型名称，点击右上角“保存”；密钥在对应“API Key”页面单独保存。

- 转写：`POST /audio/transcriptions`，multipart 字段为 `model`、`response_format=json`、`language`、可选 `prompt` 与 M4A `file`，返回 `{ "text": "..." }`。
- 润色：`POST /chat/completions`，`stream=false`，读取 `choices[0].message.content`。原文与词典编码为 JSON 数据，并通过系统提示词约束为文本编辑任务。
- 地址可填写包含 `/v1` 等前缀的基础地址，也可填写完整接口地址；不要把网页地址当作模型接口。
- 只接受 HTTPS 地址。含账号、密码、查询参数的地址会被拒绝，HTTP 重定向也不会被跟随。
- 音频上限为 24 MB；云端录音最长 180 秒，Apple 实时识别最长 55 秒；单次润色原文最多 16,000 字符。
- 兼容服务需支持上述字段、M4A 输入和 JSON 响应。其他协议需增加独立适配器，不能仅通过更换地址接入。

云端请求不经过 OpenLess 自有服务器。音频或文字仅在用户选择对应云端能力并发起操作时发送至配置的服务；Apple 非离线识别可能由 Apple 处理音频。词典提示最多使用保存顺序中的前 100 个词条。

## 跨应用输入

1. 系统设置 → 通用 → 键盘 → 键盘 → 添加新键盘 → OpenLess。
2. 在 OpenLess 键盘设置中启用“允许完全访问”。
3. 在主应用完成听写，点击“发送到键盘”。
4. 切回目标应用，长按地球图标切到 OpenLess，必要时点“刷新”，再点文字插入。

键盘只有共享文字列表、刷新、切换输入法、空格、换行、删除和收起操作；不会在其他应用中启动麦克风，也不会读取剪贴板或上传输入框内容。键盘进程仅读取共享文件，主应用是唯一写入方。

iOS 键盘扩展没有麦克风访问能力，因此此版使用主应用录音后回到键盘插入的流程。密码输入框、电话输入框及禁用扩展的应用可回退至主应用复制。项目没有使用悬浮窗、辅助功能注入、私有 API、扩展跳转主应用的 responder-chain 技巧或持续后台录音。

`openless://dictate` 可从快捷指令的“打开 URL”动作打开听写页；应用会让用户确认后开始录音。键盘本身不调用该 URL，也不会尝试自动跳回某个来源应用。

## 数据与生命周期

- `Application Support/OpenLess/openless.json`：版本化的 UTF-8 JSON，包含设置、词典、自定义风格、历史和未完成草稿；原子写入并启用文件保护。
- `Application Support/OpenLess/Recordings/<UUID>.m4a`：云端识别用录音；转写成功且文字落盘后删除。转写失败保留，主应用支持重试或清空草稿。Apple 实时识别不保存音频文件。
- App Group 的 `keyboard-clips.json`：最多 10 条由用户主动发送的文字。历史和词典不会全量共享。删除关联历史时会尝试清理对应暂存；设置页可单独清空全部键盘暂存。
- 系统钥匙串：使用 `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`，不启用 iCloud 同步或扩展共享。读取出错不会伪装成“未配置”。
- 草稿周期性保存；后台切换时保存当前草稿并结束正在进行的录音。没有声明后台音频模式；网络任务被系统挂起时，保留的录音可在返回应用后重试。
- 本地文件损坏或版本不兼容时停止覆盖原文件并显示错误。处理取消、云端错误和输出截断均保留可用的原稿；不会自动重试产生新的云端请求。

系统备份是否包含应用容器由 iOS 和用户的备份设置决定；“本地保存”不等于禁用系统设备备份。隐私清单声明未引入追踪与开发者遥测；若后续新增第三方 SDK、账号服务或数据收集，应同步修改清单和发布信息。

## 平台范围

这份 iOS 首版提供上述原生听写主流程。桌面端的 Rust provider 全目录、风格市场、GitHub 登录与云同步、局域网遥控、选区问答、全局快捷键和本地大模型运行时没有在这里接入。`contract/backend-2.0.json` 继续描述现有 Rust Host 合同，不能用它推断本 Swift 客户端已具备所有桌面功能。

平台依据：[Apple 自定义键盘限制](https://developer.apple.com/library/archive/documentation/General/Conceptual/ExtensibilityPG/CustomKeyboard.html)、[键盘完全访问](https://developer.apple.com/documentation/uikit/configuring-open-access-for-a-custom-keyboard)、[设备端识别要求](https://developer.apple.com/documentation/speech/sfspeechrecognitionrequest/requiresondevicerecognition)。
