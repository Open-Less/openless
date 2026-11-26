# OpenLess iOS 构建与签名指南

[English](README.md) | [简体中文](README.zh.md)

本目录包含 OpenLess 的原生 Swift iPhone 应用、中英文自定义键盘扩展、录制小组件以及 `CoreHost` Rust 桥接层。终端用户下载与安装说明见官网：<https://openless.top/zh/docs/installation/>。

## 平台与版本基线

- **系统版本要求**：主应用与键盘扩展支持 **iOS 16.0+**；`RecordingWidget`（录制小组件 / 实时活动）需要 **iOS 18.0+**。
- **架构与语言**：`arm64`，Swift 6.0（严格并发检查），C++17（用于 `RimeBridge`）。
- **独立版本号**：iOS 端版本由 `Config/Version.xcconfig` 独立管理（被 `Config/Base.xcconfig` 引入）。
- **默认标识**：Bundle ID 为 `app.openless.ios`，App Group 为 `group.app.openless.ios`（自定义签名时，主应用、键盘扩展与小组件三者必须共用同一个 App Group）。

## 输入模式与运行时边界

- **键盘模式**：在 OpenLess 设置中可在「语音优先布局」或「一体中英文键盘（支持长按空格听写、翻译及发送/取消手势）」之间切换。
- **核心引擎**：中英文输入由 `RimeBridge`（Objective-C++）驱动，本地离线语音由 `Packages/LocalSpeech`（SenseVoice）提供，云端 ASR / LLM / 多模态 / DeepSeek 大统一由 `CoreHost`（Rust 静态库 `libOpenLessCore`）接入 `openless-core`。
- **隐私与安全边界**：
  - 所有服务 API Key 保存在主应用的 iOS Keychain 中。
  - App Group 共享邮箱（`Packages/OpenLessShared`）仅传递受控指令与文本结果，**不传输原始音频**。
  - 打字历史与 Rime 自学习数据保留在键盘扩展容器内；结果回填前会校验文档身份，避免旧结果重复写入。
  - 空闲状态下不常驻麦克风采集或音频渲染循环。画中画（PiP）/ 返回宿主应用的兼容适配均在运行时检测并支持降级，源码构建不构成 App Store 审核通过承诺。

## 从源码构建

### 环境依赖

- macOS，已安装支持 iOS 16+ SDK 的 Xcode 与 Command Line Tools
- Rust 工具链及 iOS 目标（`aarch64-apple-ios`、`aarch64-apple-ios-sim`）
- Python 3、用于编译 Rime 依赖的 CMake / C++ 工具链，以及 XcodeGen（由 `scripts/resolve-xcodegen.sh` 自动解析，或通过 `OPENLESS_XCODEGEN_BIN` 指定）

### 完整真机构建与未签名 IPA 打包

`project.yml` 是 `OpenLess.xcodeproj` 的唯一定义源，修改工程配置前应先修改 `project.yml` 再重新生成工程。Core 构建脚本会在隔离副本中准备并打补丁，不会直接改写跨平台共享源码。

```bash
cd openless-all/ios
./scripts/bootstrap.sh
./scripts/bootstrap-local-speech.sh
./scripts/build-rime.sh
./scripts/build-core.sh
python3 scripts/collect-rust-licenses.py
python3 scripts/validate-project.py
./scripts/build.sh device Release unsigned
python3 scripts/package-ipa.py DerivedData/Build/Products/Release-iphoneos/OpenLess.app \
  "$HOME/Downloads/OpenLess-unsigned.ipa"
```

- `package-ipa.py` 会校验主应用、键盘扩展、小组件、最低系统版本、资源校验和、归档结构与可执行权限，且不会覆盖已存在的同名 IPA。
- 未签名 IPA 供个人开发者自行签名后安装到设备。

### 模拟器构建

```bash
cd openless-all/ios
./scripts/build.sh simulator Debug
```

可通过 `OPENLESS_DERIVED_DATA` 指定派生数据目录，通过 `OPENLESS_XCODEGEN_BIN` 指定 XcodeGen 可执行文件。

### 个人证书签名（真机安装）

1. 复制 `Config/Local.xcconfig.example` 为 `Config/Local.xcconfig`（已被 Git 忽略，不会提交）。
2. 在其中填写你的 Apple Development Team、唯一的 `OL_BUNDLE_ID` 与对应的 `OL_APP_GROUP`（主应用、键盘扩展、小组件三者保持一致）。
3. 执行签名构建：

```bash
cd openless-all/ios
./scripts/build.sh device Personal signed
```

## 在 iPhone 上启用键盘

1. 安装签名后的应用并先打开 **OpenLess** 主应用，授予麦克风权限并按官网指南完成语音/模型配置（<https://openless.top/zh/docs/setup/>）。
2. 打开系统 **设置 → 通用 → 键盘 → 键盘 → 添加新键盘 → OpenLess**。
3. 点击键盘列表中的 **OpenLess**，开启 **允许完全访问**（用于经 App Group 读取共享配置与触发语音控制）。
4. 在 OpenLess 设置中选择键盘形态，并在应用内输入框测试听写效果。

## 测试与验证

```bash
cd openless-all/ios
./scripts/test.sh unit
python3 scripts/test-interface-resources.py
python3 scripts/validate-project.py
IOS_SIMULATOR_ID=<dedicated-simulator-id> ./scripts/test.sh ui
```

- `unit` 覆盖共享契约、偏好配置、结果回填保障、移动端 Core 与 Rust 宿主、Rime 组词、预测与音频转换。
- `ui` 验证已安装的键盘扩展与主应用（因会修改系统键盘设置，请使用专用模拟器）；`./scripts/test.sh all` 会依次运行两套测试。
- 真实麦克风采集、音频路由与画中画保持能力仍需在真机上验证。

## 目录结构

| 目录 | 内容 |
| --- | --- |
| `App/` | 原生 SwiftUI 主应用、设置页、语音链路、录音与 Core 桥接 |
| `Keyboard/` | 键盘布局、候选词栏、长按空格语音手势与文本插入 |
| `RecordingWidget/` | iOS 18+ 录制控制与实时活动界面 |
| `PlatformShared/` | 共享原生控件与 App Group 容器访问工具 |
| `Packages/OpenLessShared/` | 版本化邮箱、偏好、手势与结果投递契约 |
| `Packages/LocalSpeech/` | 本地 SenseVoice 语音推理适配 |
| `CoreHost/` | iOS Rust workspace、C ABI 桥接与隔离 Core 补丁 |
| `RimeBridge/` | Objective-C++ Rime 输入引擎与词库桥接 |
| `Resources/` | 品牌图标资源、`en.lproj` / `zh-Hans.lproj` 本地化、输入方案、隐私清单与 `Licenses/` |
| `AppTests/`、`UITests/`、`Tests/` | 原生单元测试、扩展 UI 测试与运行时夹具 |
| `scripts/` | 依赖拉取、Rime/Core 编译、IPA 打包与工程校验脚本 |

所有外部依赖版本均固定在 `scripts/bootstrap.sh`、`scripts/bootstrap-local-speech.sh` 与 `scripts/build-predictions.py` 中。分发应用时请完整保留 `Resources/Licenses/`，如需刷新 Rust 依赖署名清单请运行 `python3 scripts/collect-rust-licenses.py`。
