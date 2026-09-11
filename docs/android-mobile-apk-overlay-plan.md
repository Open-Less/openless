# Android APK 与悬浮窗实施计划

状态：实施中（后端分层已落地）；更新：2026-09-07（以源码为准重写）。

## 1. 现有管线（源码锚点）

| 组成 | 位置 |
| --- | --- |
| Kotlin 宿主 | `android/kotlin/`（含 `androidTest/`、`test/`） |
| AIDL 桥接 | `android/aidl/com/` |
| Manifest 合成 | `android/manifests/`（基础）+ `scripts/merge-android-updater-manifest.mjs`、`merge-android-overlay-manifest.mjs`、`merge-android-v1-manifest.mjs` |
| Shizuku | `scripts/merge-android-shizuku-manifest.mjs`、`patch-android-shizuku-deps.mjs`（含各自 `.test.mjs`） |
| 前端片段 | `android/frontend/`（经 vite 别名 `@android` 被 `src/` 引用：`AndroidPermissionsPanel`、`androidMicrophonePermission`、`androidIpc`、`androidTypes`） |
| Rust 桥接 | `src-tauri/src/android/`（jni/native_bridge/insert/overlay/shizuku/updater 等） |
| CI | `.github/workflows/android-apk.yml`；打包脚本 `scripts/copy-android-scaffolding.mjs`、`configure-android-release-signing.mjs` |
| 更新器 | 公钥检查 `scripts/check-android-updater-pubkey.mjs`；manifest updater 合成 |

## 2. 契约与边界测试（npm test 自动执行）

`scripts/android-ipc-import-boundary.test.mjs`（pretest 强制）+ `android-accessibility-{enabled-detection,paste-cache,selection-ipc}-contract`、`android-credential-keystore-contract`、`android-insert-tier-fallback-contract`。IPC 面与 `contract/backend-2.0.json` 的 `androidJni` 对齐。

## 3. 剩余工作

- 悬浮窗（overlay）与无障碍链路的真机验收（契约测试之外的真实设备证据）。
- Shizuku 插件路径在真机的安装/授权/失败反馈闭环。
- 发布签名配置的密钥管理与出包验证（`configure-android-release-signing.mjs` 流程化）。

## 4. 参考

- Tauri Android：<https://v2.tauri.app/develop/mobile/>
- 主听写链路：`src-tauri/src/coordinator/dictation.rs`
- Windows IME unavailable 模式对照：`src-tauri/src/windows_ime_profile.rs`
