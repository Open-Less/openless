# OpenLess 开发者全平台构建指南

[English](README.md) | [简体中文](README.zh.md)

本目录包含 OpenLess 的跨平台客户端源码。开始构建前建议先阅读 [文档总目录](../docs/index.md) 与 [架构与模块说明](../docs/architecture.md)。面向终端用户的安装与配置手册请访问官网文档：<https://openless.top/zh/docs/>。

## 目录结构

| 路径 | 职责 |
| --- | --- |
| `app/crates/openless-core` | 框架无关的共享 Rust 核心（听写引擎、ASR / LLM / 多模态 / DeepSeek 大统一管线、风格包、词典、加密云同步与 `2.0.0` 后端契约）。 |
| `app/crates/micyou-dsp`、`app/crates/micyou-infer` | 音频 DSP 前处理与推理支持库。 |
| `app/crates/openless-capsule`、`app/native-capsule` | 原生语音胶囊状态与平台渲染支撑。 |
| `app/src` | Tauri 桌面与 Android 共用的 React + TypeScript 前端界面。 |
| `app/src-tauri` | macOS、Windows 与 Android 的 Tauri 宿主、原生适配层及内置 vendored 依赖（`src-tauri/vendor/`）。 |
| `app/ask-window` | 桌面端独立「随便问」窗口界面与桥接代码。 |
| `linux/egui` | Linux 原生宿主与 `egui` / `eframe` 界面（不依赖 Tauri 或 WebKitGTK）。 |
| `android` | Android 宿主打包、清单合并与原生胶水代码。 |
| `harmony` | HarmonyOS 原生 ArkUI 宿主及 `harmony/rust` Node-API 桥接。 |
| `app/mobile-ime` | Android 与 HarmonyOS 共享的移动端原生输入法核心。 |
| `windows/ime` | Windows TSF / 输入法注册与测试脚本。 |
| `windows/nsis`、`windows/wix`、`windows/scripts` | Windows 安装器资产与构建、验收工具。 |
| `macos/dmg`、`macos/scripts`、`macos/*.plist` | macOS 安装盘布局、构建与验收工具及应用权限配置。 |
| `linux/fcitx5-plugin` | Linux 听写落字使用的 Fcitx5 插件。 |
| `ios/` | 原生 Swift iPhone 应用、中英文键盘扩展、录制小组件与 `CoreHost` Rust 桥接。 |

> **无需初始化 Git 子模块**：`app/src-tauri/vendor/` 中已直接包含 `cpal` 与 `tauri-runtime-wry` 的定制源码，克隆仓库后即可直接构建。

## 环境要求与快速启动（桌面 Tauri）

- **Node.js**：22 及以上
- **Rust**：1.99 及以上（workspace 各 crate 的 `rust-version = "1.99"`）
- **操作系统依赖**：macOS 需安装 Xcode Command Line Tools；Windows 需安装 Visual Studio Build Tools 与 Windows SDK（或 MinGW 工具链）。

在 `openless-all/app` 目录启动桌面端开发模式：

```bash
cd openless-all/app
npm ci
npm run tauri dev
```

客户端拉起后，建议先在空白输入框中试录一句话，确认录音转写与光标插入链路正常后再着手代码改动。

常用本地测试与构建检查：

```bash
cd openless-all/app
npm test
npm run build
cargo test --locked -p openless-core
```

## 共享后端与 Linux 宿主

根 Rust workspace（`openless-all/app/Cargo.toml`）包含 `openless-core`、`micyou-dsp`、`micyou-infer`、`openless-capsule`、`native-capsule`、`linux-egui` 与 `harmony/rust`，排除 `src-tauri`，因此 Core 与 Linux 检查不依赖 Tauri 或 WebKitGTK。

`linux-egui` 在进程内直接调用 `openless_core::contract` 类型化 Rust 接口，常驻宿主与界面子进程经 Unix Socket（JSONL v2）交换状态快照和事件，并通过 Fcitx5 插件（`linux/fcitx5-plugin`）落字。

Linux 本地测试与打包：

```bash
cd openless-all/app
cargo test --locked -p openless-linux-egui
OPENLESS_LINUX_VERSION="$(node -p "require('./package.json').version.split('+')[0]")-$(cat ../linux/egui/package-revision)" bash ../linux/scripts/package-linux-egui.sh
bash ../linux/scripts/verify-linux-egui-packages.sh
```

> Linux 自动化 CI 与发布工作流目前保持暂停；本地脚本仍可产出经校验的 `.deb` / `.rpm` 与 Fcitx5 插件（已停用 AppImage）。对外分发前请参照 [发布清单](../RELEASING.zh.md) 完成检验。

## macOS 构建

请使用仓库内置构建脚本，不要直接执行裸 `tauri build`：

```bash
cd openless-all/app
INSTALL=0 ./../macos/scripts/build-mac.sh
```

产出文件：

- `openless-all/app/src-tauri/target/release/bundle/macos/OpenLess.app`
- `openless-all/app/src-tauri/target/release/bundle/dmg/OpenLess_<version>_aarch64.dmg`（Apple Silicon）或 `OpenLess_<version>_x64.dmg`（Intel）

若需构建后直接安装到本机用于开发调试，可直接运行 `./../macos/scripts/build-mac.sh`。未配置 Apple 正式签名证书时，本地脚本会自动使用 ad-hoc 签名。

## Windows 构建

构建前先运行环境预检脚本，缺少 MSVC、Windows SDK 或 MinGW 依赖时会给出明确提示：

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-preflight.ps1
```

### MSVC 路线（推荐）

通过 Visual Studio Installer 安装 `Microsoft.VisualStudio.Workload.VCTools`、MSVC v143 x64/x86 生成工具及包含 `kernel32.lib` 的 Windows 10/11 SDK。在 Developer PowerShell（或执行 `vcvars64.bat` 后）运行：

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-package-msvc.ps1
```

### GNU / MinGW 路线

无法安装 MSVC / Windows SDK 时可使用 GNU 工具链（无空格的 `openless-all` 目录路径可避免 MinGW 路径转义问题）：

```powershell
cd openless-all/app
scoop install rustup mingw
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup target add x86_64-pc-windows-gnu
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-preflight.ps1 -Toolchain gnu
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-build-gnu.ps1
```

产出文件：

- `%TEMP%\openless-windows-gnu\app\src-tauri\target\x86_64-pc-windows-gnu\release\openless.exe`
- `%TEMP%\openless-windows-gnu\app\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\msi\OpenLess_*_x64_en-US.msi`
- `%TEMP%\openless-windows-gnu\app\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\nsis\OpenLess_*_x64-setup.exe`

### 热键注入门禁与运行时说明

改动 Windows 热键逻辑且暂时无法进行实体键盘回归时，可运行干跑注入门禁：

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-hotkey-injection-smoke.ps1 -ExePath .\.artifacts\windows-gnu\dev\openless.exe
```

- Windows 不需要 macOS 的辅助功能权限，可在「设置 → 权限 → 全局热键」查看监听状态。
- 麦克风权限通过打开短时输入流校验，不会仅凭设备格式查询误判为已授权。
- 通过 `Ctrl+V` 写入时，在无法确认落字成功前按剪贴板回退处理。

## Android 构建

Android SDK/NDK 依赖、脚手架生成、多架构 APK（`arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86`）构建与真机验证见 [Android 构建指南](android/README.zh.md#构建与-ci)。

## iOS 构建

iOS 端（`ios/`）为独立的原生 Swift + Rust 工程，应用与键盘面向 iOS 16+（录制小组件需 iOS 18+），采用独立版本号。完整依赖拉取、`CoreHost` 与 Rime 构建、个人证书签名、模拟器调试与未签名 IPA 打包步骤见 [iOS 构建指南](ios/README.zh.md#从源码构建)。

## 发布签名与分发

正式版与 Beta 版的版本号同步、签名公证、CDN 更新清单及发布校验流程见 [维护者发布清单](../RELEASING.zh.md)。

## 第三方组件与字体署名

OpenLess 改编了 **Rare UI** 的部分组件（GitHub Activity、Password Input、Toast、AI Prompt Box / Streaming Response，Copyright (c) 2026 Swami Malode，MIT + Commons Clause + Attribution），并内置 **Open Runde** 字体（SIL Open Font License 1.1）。完整署名与许可文件见 [组件与字体署名说明](../docs/ATTRIBUTION.md)，应用内亦可在「设置 → 关于与更新 → 组件来源」查看。

## 忽略的本地产出

以下构建产物默认不纳入版本控制：

- `app/node_modules/`
- `app/dist/`
- `app/target/`
- `app/src-tauri/target/`
- `app/src-tauri/gen/`
- `.DS_Store`
