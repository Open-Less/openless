# OpenLess 全平台

[English](README.md) | [简体中文](README.zh.md)

本目录包含跨平台的 OpenLess 应用。建议先阅读仓库的[文档入口](../docs/index.md)与[架构和接口调用](../docs/architecture.md)。

## 原生 iOS 应用与键盘

[`ios/`](ios/README.zh.md) 包含原生 iPhone 应用、中英文键盘扩展以及录音小组件。设置中提供以语音为核心的布局，或单键盘模式（长按空格听写、翻译、发送/取消手势）。应用与键盘面向 iOS 16；录音小组件要求 iOS 18。iOS 指南涵盖共享的版本配置、输入引擎、从源码构建、签名以及未签名 IPA 打包。

## 应用目录

可运行的源码位于 `app/`：

- `app/crates/openless-core`：与框架无关的共享后端接口与业务规则；
- `app/src`：供各 Tauri 宿主使用的 React 前端；
- `app/src-tauri`：macOS、Windows 与 Android 的 Tauri 宿主及原生适配器；
- `app/linux-egui`：Linux 宿主与 egui/eframe 界面，不依赖 Tauri 或 WebKitGTK；
- `app/harmony`：HarmonyOS 宿主（ArkTS + ArkWeb），配合共享 Rust 后端；
- `app/mobile-ime`：Android 与 HarmonyOS 共用的原生输入法后端；
- `app/scripts/linux-fcitx5-plugin`：Linux 听写所经过的 fcitx5 插件。

Tauri 清单包含 `app/src-tauri/vendor/` 下的路径依赖（`cpal`、`tauri-runtime-wry`）。这些 crate 已随仓库签入，因此**没有需要初始化的 Git 子模块**——新克隆的仓库即可直接构建。根 Core/Linux 工作区排除了 `src-tauri`，因此其独立检查不需要它们。

使用 Node.js 22 和 Rust 1.99 或更新版本。以下命令启动桌面 Tauri 宿主。Android 的平台工程生成步骤见下文。

```bash
cd app
npm ci
npm run tauri dev
```

## 共享后端与 Linux 宿主

本仓库提供共享的类型化 Rust 接口、语义事件、fixtures 以及 Linux 适配器。`linux-egui/src/main.rs` 现已实现 egui/eframe 应用，由 `LinuxHost` 和 `LinuxBackendBuilder` 将其接入 Core。接入方式见 [Linux 宿主接口](../docs/development/linux.md)，剩余工作见 [Linux 开发需求](../docs/requirements.md#linux)。

```bash
cd app
cargo test -p openless-core --locked
cargo test -p openless-linux-egui --all-targets --locked
pwsh ./scripts/check-core-deps.ps1
pwsh ./scripts/check-core-deps.ps1 openless-linux-egui
```

真机和安装验证完成前，Linux 打包为手动进行：专用 CI 工作流（`check-linux-egui.yml`、`release-linux-egui.yml`）已移除，桌面版发布也不再分发 Linux。本地脚本仍可在不依赖 WebKitGTK 的情况下构建 deb/rpm 与 fcitx5 插件（AppImage 已弃用）。恢复自动化 Linux 发布需要 [`RELEASING.md`](../RELEASING.md) 和 [Linux 开发需求](../docs/requirements.md#linux) 中所述的验收证据。

## macOS 构建

请使用项目的构建脚本，而不要直接调用 `tauri build`：

```bash
cd app
INSTALL=0 ./scripts/build-mac.sh
```

生成的 macOS 产物：

- `app/src-tauri/target/release/bundle/macos/OpenLess.app`
- `app/src-tauri/target/release/bundle/dmg/OpenLess_<version>_aarch64.dmg`（Apple Silicon）或 `OpenLess_<version>_x64.dmg`（Intel）

开发期间进行本地安装：

```bash
cd app
./scripts/build-mac.sh
```

## Windows 构建

可运行的 Tauri 应用仍是 `app/`。Windows 贡献者在构建前应先运行预检，以便在缺少 MSVC、Windows SDK 或 MinGW 工具时给出可操作的错误信息。

```powershell
cd app
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1
```

### MSVC 路线

当已安装 Visual Studio Build Tools 与 Windows SDK 时使用此路线。打开 Developer PowerShell，或调用 `vcvars64.bat`，然后运行：

```powershell
cd app
powershell -ExecutionPolicy Bypass -File .\scripts\windows-package-msvc.ps1
```

Visual Studio Installer 必需组件：

- `Microsoft.VisualStudio.Workload.VCTools`
- MSVC v143 x64/x86 build tools
- 提供 `kernel32.lib` 的 Windows 10/11 SDK

如果缺少 `link.exe` 或 `kernel32.lib`，请重新运行：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1 -Toolchain msvc
```

### GNU / MinGW 路线

当 MSVC/Windows SDK 不可用时使用此路线。应用现已位于不含空格的 `openless-all` 目录下，以避免生成导入库时出现 GNU/MinGW 路径引号问题。请使用辅助脚本，以保持 GNU 构建环境与目标设置一致。

```powershell
cd app
scoop install rustup mingw
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup target add x86_64-pc-windows-gnu
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1 -Toolchain gnu
powershell -ExecutionPolicy Bypass -File .\scripts\windows-build-gnu.ps1
```

生成的 GNU 产物：

- `%TEMP%\openless-windows-gnu\src-tauri\target\x86_64-pc-windows-gnu\release\openless.exe`
- `%TEMP%\openless-windows-gnu\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\msi\OpenLess_*_x64_en-US.msi`
- `%TEMP%\openless-windows-gnu\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\nsis\OpenLess_*_x64-setup.exe`

### 热键注入门禁

当无法用物理键盘做回归验证时，在修改 Windows 热键前后使用此门禁。它通过协调器的 `handle_pressed` / `handle_released` 路径注入仅用于开发/测试的热键点击，断言日志中包含 `[coord] hotkey pressed`，并自动取消试运行会话。

```powershell
cd app
powershell -ExecutionPolicy Bypass -File .\scripts\windows-hotkey-injection-smoke.ps1 -ExePath .\.artifacts\windows-gnu\dev\openless.exe
```

### Windows 运行时说明

- Windows 不需要 macOS 的辅助功能权限。通过 设置 -> 权限 -> 全局热键 查看监听器状态。
- 麦克风权限通过打开一个短时输入流来检查，因此仅查询设备格式不会被当作已获得权限。
- 通过 `Ctrl+V` 的文本插入被视为复制回退方案，除非应用能够确认插入成功。

## Android 构建

Android 开发环境、脚手架合成、APK 构建、ABI 与真机验证见 [Android 构建指南](app/android/README.zh.md#构建与-ci)。

## HarmonyOS 构建

ArkTS/ArkWeb 宿主通过前端脚本构建；工具链细节参见 [`app/harmony/README.zh.md`](app/harmony/README.zh.md)。

```bash
cd app
bash scripts/build-harmony-frontend.sh
```

要求：HarmonyOS 命令行工具、Python 3，以及 `rustup target add aarch64-unknown-linux-ohos`。未签名的 HAP 输出到 `app/harmony/openless-project/entry/build/default/outputs/default/`；`release-harmony.yml` 工作流会将其附加到发布中，而 AppGallery 安装包则需手动提交。真机安装还需要 `openless.hm.com` 的华为调试证书和包含该设备的调试 Profile，详见 [`app/harmony/README.zh.md`](app/harmony/README.zh.md)。

## iOS 构建

iOS 使用独立版本。工具链、源码构建、签名、设备安装与未签名 IPA 打包统一维护在 [iOS 工程指南](ios/README.zh.md#从源码构建)。

## 发布签名

Beta 和稳定版的签名、公证与发布验收见[发布规范](../RELEASING.zh.md)。macOS 本机打包在没有 Apple 签名材料时会使用临时签名。

## 第三方组件

概览页的年度活动卡片移植自 [Rare UI GitHub Activity](../docs/ATTRIBUTION.md#rare-ui-github-activity)（Copyright (c) 2026 Swami Malode）。许可是 MIT 加 Commons Clause。格子尺寸、间距、圆角、内边距、月份标签、悬浮提示和可展开面板保持上游组件；本地听写次数替换 GitHub 提交，语音输入最多的软件替换仓库。应用内署名在「设置 → 关于与更新 → 组件来源」。详见 [docs/ATTRIBUTION.md](../docs/ATTRIBUTION.md)。

## 忽略的本地输出

以下内容有意仅保留在本地：

- `app/node_modules/`
- `app/dist/`
- `app/target/`
- `app/src-tauri/target/`
- `app/src-tauri/gen/`
- `.DS_Store`
