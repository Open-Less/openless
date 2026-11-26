# OpenLess Android 平台代码

[English](README.md) | [简体中文](README.zh.md)

Android 应用通过 Tauri WebView 提供设置，键盘、悬浮球和无障碍集成由原生 Kotlin 服务提供。运行行为见 [移动端输入法调用链](../../../docs/architecture.md#移动端输入法)。

## 源码定位

| 路径 | 职责 |
| --- | --- |
| [`kotlin/`](kotlin/) | 键盘、常驻运行时、预热 Activity、悬浮球、无障碍、Shizuku 和原生测试 |
| [`aidl/`](aidl/) 与 [`manifests/`](manifests/) | Binder 接口、Android 权限与服务、XML 资源 |
| [`frontend/`](frontend/) | 权限、麦克风和悬浮球控件，通过 Vite `@android` 别名导入 |
| [`../src-tauri/src/android/`](../src-tauri/src/android/) | Rust JNI、插入回退、权限和更新器。共用偏好类型在 Core 的 [`android_types.rs`](../crates/openless-core/src/android_types.rs) |
| [`../mobile-ime/`](../mobile-ime/README.zh.md) | 共享 librime 引擎、词库、候选确认、学习与隐私规则 |

## 构建与 CI

准备 Node.js 22、Rustup 和目标 Android target、JDK、Android SDK 与 NDK、Python 3、CMake。CI 固定版本见 [APK 工作流](../../../.github/workflows/android-apk.yml)。在 `openless-all/app` 执行，先配置 `ANDROID_HOME` 和 `ANDROID_NDK_HOME`。若 `/data-win/openless-cache` 不可用，将 `OPENLESS_CACHE_ROOT` 指向可写目录。

```bash
npm ci
npm run tauri -- android init --ci
npm run copy:android-scaffolding
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
node scripts/merge-android-updater-manifest.mjs
npm run tauri:android:build:debug
```

复制脚本把 Kotlin、AIDL 和资源放入生成的 `src-tauri/gen/android/` 工程。需要持久化的改动写在仓库的 Android 源码和合成脚本中。debug 和 release 包装命令为目标 ABI 准备原生库和 Rime 离线词库。可单独运行 `python3 scripts/build-mobile-rime.py android --ndk "$ANDROID_NDK_HOME"` 检查 Rime。直接运行 Gradle 且缺少原生输入时，先带 `--stage` 准备资源。已有同版本 Rust 库时，仅修改 Kotlin 的 arm64 调试可在 `src-tauri/gen/android` 中执行 `./gradlew app:assembleArm64Debug -x app:rustBuildArm64Debug`。

使用 `npm run tauri:android:build:release` 构建 release 配置。[APK 工作流](../../../.github/workflows/android-apk.yml)按 ABI 收集成品并处理发布签名。本机安装测试时用 `adb shell getprop ro.product.cpu.abi` 选择匹配的 APK。签名兼容时可用 `adb install -r <apk>` 升级，调试包和正式包的签名可能不同。

应用目录中的 `npm test` 覆盖 IPC、清单、权限和插入契约。在匹配架构的真机上验证键盘手势、冷启动、悬浮球权限、无障碍和 Shizuku。正式出包遵循[发布规范](../../../RELEASING.zh.md)。
