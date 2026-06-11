# OpenLess Android 平台代码

Android 相关 Rust、Kotlin 与前端代码的统一入口。桌面端通过 `#[cfg(not(mobile))]` 分层，不受影响。

## 目录结构

```text
android/
├── kotlin/              # Kotlin 模板（CI 复制到 gen/android/）
├── manifests/           # AndroidManifest snippet + res/xml
└── frontend/            # React 模块（Vite 别名 @android）

src-tauri/src/android/   # Rust 运行时模块（crate::android）
```

## Rust（`src-tauri/src/android/`）

| 模块 | 职责 |
|------|------|
| `jni.rs` | JNI 工具（clipboard、overlay service、accessibility） |
| `native_bridge.rs` | Kotlin ↔ Coordinator JNI 入口 |
| `overlay.rs` | 悬浮窗权限与 show/hide |
| `accessibility.rs` | 无障碍服务状态与 paste |
| `insert.rs` | 跨 App 文本插入策略 |
| `types.rs` | Android 偏好与状态类型 |

主 crate 通过 `mod android;` 引入，常用 API 经 `crate::android::` 扁平 re-export。

## Kotlin（`android/kotlin/`）

`tauri android init` 后由 [`scripts/copy-android-scaffolding.mjs`](../scripts/copy-android-scaffolding.mjs) 复制到 `src-tauri/gen/android/app/src/main/java/com/openless/app/`。

Manifest 合并脚本：

- [`scripts/merge-android-v1-manifest.mjs`](../scripts/merge-android-v1-manifest.mjs) — 麦克风权限（`android/manifests/AndroidManifest.v1.snippet.xml`）
- [`scripts/merge-android-overlay-manifest.mjs`](../scripts/merge-android-overlay-manifest.mjs) — 悬浮窗 / 无障碍

## 前端（`android/frontend/`，别名 `@android`）

| 路径 | 职责 |
|------|------|
| `lib/androidTypes.ts` | Android 偏好与状态 TS 类型 |
| `lib/androidIpc.ts` | overlay / accessibility Tauri invoke |
| `lib/androidMicrophonePermission.ts` | WebView 麦克风权限辅助 |
| `components/AndroidPermissionsPanel.tsx` | 设置页 Android 权限与 overlay 配置 |

`src/lib/types.ts` 与 `src/lib/ipc.ts` 保留 re-export，现有 import 路径仍可用。

## 构建与 CI

**CI（overlay / 无障碍 ADB 测试 APK）** — 合并 v1 麦克风权限 + overlay / 无障碍 manifest，用于真机 ADB 测试完整悬浮窗与无障碍能力（非仅应用内听写）：

```bash
cd openless-all/app
npm ci && npm run build
CI=true npm run tauri -- android init --ci
node scripts/copy-android-scaffolding.mjs
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
CI=true npm run tauri:android:build
```

Workflow： [`.github/workflows/android-apk.yml`](../../.github/workflows/android-apk.yml)

**本地 overlay / 无障碍开发（v3）** — 与 CI 相同的 manifest 合并链，使用本地 init / copy 脚本：

```bash
cd openless-all/app
npm run tauri:android:init
npm run copy:android-scaffolding
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
npm run tauri:android:build
```

## 相关文档

- [AGENTS.md](../../AGENTS.md) — 真机闪退排查
- [docs/android-mobile-apk-overlay-plan.md](../../docs/android-mobile-apk-overlay-plan.md) — 分阶段产品计划
