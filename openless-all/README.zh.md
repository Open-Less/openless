# OpenLess 全平台

[English](README.md) | [简体中文](README.zh.md)

本目录包含跨平台的 OpenLess 应用。建议先阅读仓库的[文档索引](../docs/index.md)、[架构说明](../docs/architecture.md)和[源码结构](../docs/structure.md)。

## 原生 iOS 应用与键盘

[`ios/`](ios/README.md) 包含原生 iPhone 应用、中英文键盘扩展以及录音小组件。设置中提供以语音为核心的布局，或单键盘模式（长按空格听写、翻译、发送/取消手势）。应用与键盘面向 iOS 16；录音小组件要求 iOS 18。iOS 指南涵盖共享的版本配置、输入引擎、从源码构建、签名以及未签名 IPA 打包。

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

```bash
cd app
npm ci
npm run tauri dev
```

## 共享后端与 Linux 宿主

本仓库提供共享的类型化 Rust 接口、语义事件、fixtures 以及 Linux 适配器。`linux-egui/src/main.rs` 现已实现 egui/eframe 应用，由 `LinuxHost` 和 `LinuxBackendBuilder` 将其接入 Core。剩余的宿主/界面工作与产品验收记录在 [Linux 交接文档](../docs/linux-egui-handoff/README.md) 中。

```bash
cd app
cargo test -p openless-core --locked
cargo test -p openless-linux-egui --all-targets --locked
pwsh ./scripts/check-core-deps.ps1
pwsh ./scripts/check-core-deps.ps1 openless-linux-egui
```

在 L01–L12 设备验收尚未完成期间，Linux 打包为手动进行：专用 CI 工作流（`check-linux-egui.yml`、`release-linux-egui.yml`）已移除，桌面版发布也不再分发 Linux。本地脚本仍可在不依赖 WebKitGTK 的情况下构建 deb/rpm 与 fcitx5 插件（AppImage 已弃用）。恢复自动化 Linux 发布需要 [`RELEASING.md`](../RELEASING.md) 和 [`docs/ci-trigger-and-cache-policy.md`](../docs/ci-trigger-and-cache-policy.md) 中所述的验收证据。

## macOS 构建

请使用项目的构建脚本，而不要直接调用 `tauri build`：

```bash
cd app
INSTALL=0 ./scripts/build-mac.sh
```

生成的 macOS 产物：

- `app/src-tauri/target/release/bundle/macos/OpenLess.app`
- `app/src-tauri/target/release/bundle/dmg/OpenLess_<version>_aarch64.dmg`

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
npm ci
npm run tauri -- build
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
```

### Windows 运行时说明

- Windows 不需要 macOS 的辅助功能权限。通过 设置 -> 权限 -> 全局热键 查看监听器状态。
- 麦克风权限通过打开一个短时输入流来检查，因此仅查询设备格式不会被当作已获得权限。
- 通过 `Ctrl+V` 的文本插入被视为复制回退方案，除非应用能够确认插入成功。

## Android 构建

新克隆的仓库没有生成好的 Android 工程，`src-tauri/gen/android` 不存在时构建脚本会直接报错。先初始化工程、复制 Kotlin/XML 脚手架，再按与 CI 相同的方式合并 manifest：

```bash
cd app
npm ci
npm run tauri:android:init                  # 生成 src-tauri/gen/android
npm run copy:android-scaffolding            # Kotlin 源码 + res/xml
node scripts/merge-android-v1-manifest.mjs  # 麦克风权限
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
node scripts/merge-android-updater-manifest.mjs   # release 需要的应用内更新 manifest
npm run tauri:android:build:release
```

同一条链路（含本地 Gradle 开发流程）见 [`app/android/README.zh.md`](app/android/README.zh.md)。

APK 输出到 `app/src-tauri/gen/android/app/build/outputs/apk/<abi>/release/` 下。调试构建请使用 `npm run tauri:android:build:debug`；`android-apk.yml` 工作流会为 `arm64-v8a`、`armeabi-v7a`、`x86` 和 `x86_64` 生成调试冒烟测试产物。原生键盘/输入法后端位于 `app/android/` 与 `app/mobile-ime/`。

## HarmonyOS 构建

ArkTS/ArkWeb 宿主通过前端脚本构建；工具链细节参见 [`app/harmony/README.zh.md`](app/harmony/README.zh.md)。

```bash
cd openless-all/app
bash scripts/build-harmony-frontend.sh
```

要求：HarmonyOS 命令行工具、Python 3，以及 `rustup target add aarch64-unknown-linux-ohos`。未签名的 HAP 输出到 `app/harmony/openless-project/entry/build/default/outputs/default/`；`release-harmony.yml` 工作流会将其附加到发布中，而 AppGallery 安装包则需手动提交。真机安装还需要 `openless.hm.com` 的华为调试证书和包含该设备的调试 Profile，详见 [`app/harmony/README.zh.md`](app/harmony/README.zh.md)。

## iOS 构建

原生项目独立于桌面版本；参见 [`ios/README.zh.md`](ios/README.zh.md)。要求：Apple Silicon Mac、已选择命令行工具的 Xcode、Python 3、CMake、Git 与 Rustup。

```bash
cd openless-all/ios
./scripts/bootstrap.sh                    # Rust 1.99.0、iOS target、固定版本的原生依赖、XcodeGen
./scripts/bootstrap-local-speech.sh       # 内置 SenseVoice 运行时
./scripts/build-rime.sh                   # 词库数据与编译后的码表
./scripts/build-core.sh                   # Frameworks/<platform>/libOpenLessCore.a
python3 scripts/collect-rust-licenses.py
python3 scripts/validate-project.py
./scripts/build.sh device Release unsigned
python3 scripts/package-ipa.py \
  DerivedData/Build/Products/Release-iphoneos/OpenLess.app \
  "$HOME/Downloads/OpenLess-0.1.0-16-unsigned.ipa"
```

`build.sh` 参数为 `<simulator|device> <Debug|Personal|Release> [unsigned|signed]`，默认 `simulator Debug unsigned`；在 `build-core.sh` 与 `build-rime.sh` 产出 `libOpenLessCore.a`、`libOpenLessRime.a`、Rime 码表和预测词典之前会拒绝运行。`package-ipa.py` 需要已构建的 `.app` 和一个新的输出路径，不会覆盖已有 IPA，且只打包未签名真机构建，供个人签名使用。

## 发布签名

带标签的 Tauri 发布（`v*-tauri`）必须使用 Developer ID 签名并完成公证，这样用户下载后即可直接打开 macOS 应用，无需手动移除隔离属性。Linux 安装包使用单独的手动 egui 工作流和独立的 minisign 密钥。

必需的 GitHub secrets：

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

可选的：

- `APPLE_PROVIDER_SHORT_NAME`
- `KEYCHAIN_PASSWORD`

手动触发的工作流仍可生成 ad-hoc 签名的测试构建，但若缺少签名/公证 secrets，带标签的 macOS 发布将会失败。

## 第三方组件

概览页的年度活动卡片移植自 [Rare UI GitHub Activity](https://rareui.com/components/githubactivity)（Copyright (c) 2026 Swami Malode）。许可是 MIT 加 Commons Clause。格子尺寸、间距、圆角、内边距、月份标签、悬浮提示和可展开面板保持上游组件；本地听写次数替换 GitHub 提交，语音输入最多的软件替换仓库。应用内署名在「设置 → 关于与更新 → 组件来源」。详见 [docs/ATTRIBUTION.md](../docs/ATTRIBUTION.md)。

## 忽略的本地输出

以下内容有意仅保留在本地：

- `app/node_modules/`
- `app/dist/`
- `app/target/`
- `app/src-tauri/target/`
- `app/src-tauri/gen/`
- `.DS_Store`
