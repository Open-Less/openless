# OpenLess iOS

[English](README.md) | [简体中文](README.zh.md)

原生 iPhone app、键盘扩展与录音小组件。app 使用 SwiftUI 与 UIKit；键盘结合 UIKit 控件与 KeyboardKit。主 app 负责音频采集、语音识别以及共享的 Rust Core。

## 版本与平台支持

`Config/Version.xcconfig` 是全部三个 bundle 的唯一版本来源。本次集成为 **0.1.0 (16)**，并保留 Build 15 的键盘行为。在该文件中更新 `MARKETING_VERSION` 与 `CURRENT_PROJECT_VERSION`，然后使用 XcodeGen 重新生成工程。产品校验会拒绝 app 与扩展版本不一致的情况。

| 组件 | 最低 iOS | 职责 |
| --- | --- | --- |
| `OpenLess` | 16 | 设置、语音、处理、历史、词库与样式 |
| `OpenLessKeyboard` | 16 | 输入、候选、录音手势与插入 |
| `OpenLessRecordingWidget` | 18 | 录音控制与实时活动集成 |

iOS 版本独立于桌面发布版本。设备构建当前面向 arm64 iPhone；模拟器构建面向 Apple 芯片。

## 输入模式

在设置中选择 **语音优先** 或 **单键盘**。该选择、键盘高度、中英文语言以及所选处理样式会在各键盘会话之间保持。

| 模式 | 输入与语音行为 |
| --- | --- |
| 语音优先 | 专用语音面板以及中英文输入；长按空格可移动光标或开始录音 |
| 单键盘 | 中英文输入，带 `123`、语言切换、宽空格以及宿主的 Return 操作；按住空格录音 |

两种模式都保留语音工具条、处理样式、翻译与系统键盘切换。按住录音提供取消、翻译与发送操作。单键盘模式使用一个已配置的翻译输出语言；录音时会为该次请求冻结该选择。基础输入无需完全访问即可使用。共享偏好设置与跨进程语音控制需要完全访问以及匹配的 App Group 授权。

中文组合使用 librime 与有界的 rime-ice 词典。候选会展开为网格；用户可以选择中文候选，或显式确认输入的拉丁字母。英文使用离线词频/上下文字典、`UITextChecker` 以及补充的 `UILexicon`。这些是本扩展所用的输入引擎，而非系统键盘的完整预测模型。补充条目保留在内存中。按键声音使用 `UIDevice.playInputClick()`；触感与声音偏好保持可配置。

普通语音识别使用内置的 SenseVoice Small int8。开发者设置可以选择 Apple Speech，或配置 Core ASR/LLM/Omni 提供方。识别语言与输入语言相互独立。翻译与润色取决于所配置的处理提供方。准备语音访问不会开始录音。

## 从源码构建

要求：Apple 芯片 Mac、已选择其命令行工具的 Xcode 27、Python 3、CMake、Git 与 Rustup。Bootstrap 会安装 Rust 1.99.0 与两个 iOS target，并固定原生依赖与 XcodeGen 版本。下载的语音产物与预测源会做校验和验证。

`openless-all/app/` 下的桌面端源码保持遵循 `beta` 不变。`scripts/prepare-core.py` 将检入的 Core 复制到被忽略的 `CoreHost/.build/core-source/` 目录，并在该处应用 `CoreHost/Patches/ios-input-contracts.patch`。iOS Rust workspace 构建并测试该副本。该补丁新增移动端输入契约，且不改变桌面、Android 或 Linux 所使用的 Core。Bootstrap、测试、原生构建与许可证收集会自动准备该副本；不兼容的上游变更会使补丁校验失败。

```sh
cd openless-all/ios
./scripts/bootstrap.sh
./scripts/bootstrap-local-speech.sh
./scripts/build-rime.sh
./scripts/build-core.sh
python3 scripts/collect-rust-licenses.py
python3 scripts/validate-project.py
./scripts/build.sh device Release unsigned
python3 scripts/package-ipa.py DerivedData/Build/Products/Release-iphoneos/OpenLess.app \
  "$HOME/Downloads/OpenLess-0.1.0-16-unsigned.ipa"
```

未签名 IPA 用于个人签名。打包会校验真实的 app、键盘、小组件、最低部署版本、资源校验和、归档内容与可执行权限。已存在的输出绝不会被覆盖。

模拟器请使用 `./scripts/build.sh simulator Debug`。`OPENLESS_DERIVED_DATA` 选择构建目录，`OPENLESS_XCODEGEN_BIN` 选择已安装的 XcodeGen。`project.yml` 生成检入的 Xcode 工程；请在重新生成之前编辑该规格文件。

若要在 Xcode 中进行设备签名，请将 `Config/Local.xcconfig.example` 复制为 `Config/Local.xcconfig`，设置你的 team、bundle ID 与 App Group，然后运行 `./scripts/build.sh device Personal signed`。三个 bundle 必须使用相同的 App Group。本地签名配置不进入 Git。

## 启用键盘

1. 安装已签名的 app，打开 OpenLess 以启用语音访问。
2. 在 iOS 设置中，依次进入 通用 → 键盘 → 键盘 → 添加新键盘 → OpenLess。
3. 为共享设置与语音控制启用完全访问。
4. 在 OpenLess 设置中选择键盘体验方式，并在 app 的键盘编辑器中试用。

主 app 会在需要时激活语音，并在运行时支持时返回到发起请求的宿主应用。活跃的 PiP 可以让会话在各次录音之间保持可用。隔离的 PiP/宿主返回兼容适配层使用运行时检查的私有选择器，在不可用时回退；本源码构建并不构成 App Store 审核通过的声明。空闲状态不会让麦克风或音频渲染循环保持运行。

## 验证

```sh
./scripts/test.sh unit
python3 scripts/test-interface-resources.py
python3 scripts/validate-project.py
IOS_SIMULATOR_ID=<dedicated-simulator-id> ./scripts/test.sh ui
```

单元检查覆盖共享契约、偏好设置、投递、移动端 Core 与 Rust Host、真实的 Rime 组合、预测以及音频转换。UI 测试会运行已安装的扩展与宿主 app；请使用专用模拟器，因为它们会配置键盘设置。`test.sh all` 合并两套测试。GitHub iOS workflow 会验证共享源码保持不变、运行移动端 Swift/Rust/资源测试、构建原生依赖与未签名 iPhone app，并校验 IPA。运行时的麦克风、音频路由与 PiP 行为仍需在设备上测试。

## 源码布局

| 目录 | 内容 |
| --- | --- |
| `App/` | 原生 app、设置、语音、音频与 Core 集成 |
| `Keyboard/` | 输入布局、候选、语音手势与宿主插入 |
| `RecordingWidget/` | 录音控制与实时活动视图 |
| `PlatformShared/` | 共享原生控件与容器访问 |
| `Packages/OpenLessShared/` | 带版本的邮箱、偏好设置、手势与投递契约 |
| `Packages/LocalSpeech/` | app 使用的 SenseVoice 运行时适配器 |
| `CoreHost/` | iOS Rust workspace、C ABI 以及隔离的 Core 源码补丁 |
| `RimeBridge/` | Objective-C++ 组合与词库桥接 |
| `Resources/` | 品牌资源、本地化、输入 schema、隐私清单与许可证 |
| `AppTests/`, `UITests/`, `Tests/` | 原生、扩展与运行时夹具 |
| `scripts/` | 依赖引导、构建、打包与校验 |

凭证保存在主 app 的 Keychain 中。App Group 承载有界命令与结果，而非原始音频。输入数据与 Rime 学习内容保存在键盘容器中；系统补充词不会被持久化。结果插入会校验当前文档并记录投递，以防止过期重放。

依赖来源固定在 `bootstrap.sh`、`bootstrap-local-speech.sh` 与 `build-predictions.py` 中。分发 app 时请保留 `Resources/Licenses/`。`collect-rust-licenses.py` 会从锁定的运行时依赖图刷新归属声明。

生成的 framework、模型、编译后的词典、日志、截图、签名配置与 IPA 归档均被排除在 Git 之外。源码资源、夹具、lockfile 与依赖声明是项目的一部分。
