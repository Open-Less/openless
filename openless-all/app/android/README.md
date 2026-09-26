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
| `shizuku.rs` | Shizuku 状态诊断与受控无障碍恢复 |
| `insert.rs` | 跨 App 文本插入策略 |
| `updater.rs` | 应用内更新（manifest 拉取、minisign 校验、系统安装器） |
| `updater_logic.rs` | 更新 URL / 版本比较纯函数（全平台可测） |
| `types.rs` | Android 偏好与状态类型 |

主 crate 通过 `mod android;` 引入，常用 API 经 `crate::android::` 扁平 re-export。

## Kotlin（`android/kotlin/`）

`tauri android init` 后由 [`scripts/copy-android-scaffolding.mjs`](../scripts/copy-android-scaffolding.mjs) 复制到 `src-tauri/gen/android/app/src/main/java/com/openless/app/`。

Manifest 合并脚本：

- [`scripts/merge-android-v1-manifest.mjs`](../scripts/merge-android-v1-manifest.mjs) — 麦克风权限（`android/manifests/AndroidManifest.v1.snippet.xml`）
- [`scripts/merge-android-overlay-manifest.mjs`](../scripts/merge-android-overlay-manifest.mjs) — 悬浮窗 / 无障碍
- [`scripts/merge-android-shizuku-manifest.mjs`](../scripts/merge-android-shizuku-manifest.mjs) — Shizuku Provider / 授权 Activity
- [`scripts/patch-android-shizuku-deps.mjs`](../scripts/patch-android-shizuku-deps.mjs) — Shizuku Gradle 依赖

## 前端（`android/frontend/`，别名 `@android`）

| 路径 | 职责 |
|------|------|
| `lib/androidTypes.ts` | Android 偏好与状态 TS 类型 |
| `lib/androidIpc.ts` | overlay / accessibility / Shizuku Tauri invoke |
| `lib/androidMicrophonePermission.ts` | WebView 麦克风权限辅助 |
| `components/AndroidPermissionsPanel.tsx` | 设置页 Android 权限与 overlay 配置 |

`src/lib/types.ts` 与 `src/lib/ipc.ts` 保留 re-export，现有 import 路径仍可用。

## 笔画输入法 IME 最近更新（`OpenLessImeService.kt`）

面板高度固定为 300dp（`SwipeModeContainer.onMeasure()` 强制），笔画面板内编码区 24dp + 候选区 36dp（合计 60dp）与下方按键区共同瓜分剩余高度，任何输入状态下都不重新布局。

| 功能 | 说明 |
|------|------|
| 手势 | 左右切换面板滑动阈值 72dp→100dp；新增下滑 ≥120dp 收起键盘（`hideKeyboardPanel()`）；面板切换带滑入动画（`refreshInputView(slideDirection)`） |
| 编码区/候选区 | 固定高度、扁平背景 + 分隔线（`buildEncodeAreaBackground()`），选中/首选候选字改为编码文字同款浅蓝 + 加粗（`strokeEncodeAccentColor`），不再用红色；候选区超出部分用 `showCandidateOverlay()` 悬浮层展开，不推挤按键 |
| 中间 3×4 笔画键 | 间距收紧到 ~2dp（键位 `setMargins(dp(1)...)`），并与右侧红色功能键列、左侧标点列的行边距对齐一致 |
| 键盘设置 | 长按 Logo 打开全屏原生设置页 `OpenLessKeyboardSettingsActivity`（先实现震动强度/时长，后续可继续加项） |
| 语言同步修复 | `OpenLessApplication` 原来按精确类型判断 `MainActivity`，实际设置页跑在子类 `OpenLessBackendWarmupActivity` 上从未触发，改成 `is` 判断 |
| 剪贴板 | 新增历史持久化 `OpenLessClipboardHistory.kt`，按钮配色与笔画面板统一 |
| 语音纠错联动 | `native_bridge.rs` 新增 `nativeAddCorrectionRule`，手动改过的听写结果自动写入纠错词典 |

开发流程：每次改动后用 `npm run copy:android-scaffolding` 同步 → `gradlew app:assembleArm64Debug -x app:rustBuildArm64Debug`（Kotlin-only 改动跳过 Rust 重编译）→ `adb install -r` 装机 → 通过 `adb exec-out screencap` 或用户反馈截图核对真机效果；涉及尺寸争议时用 `adb shell wm density` + 实测 px 反推 dp，避免凭空猜测布局问题。

## 构建与 CI

**CI（overlay / 无障碍 ADB 测试 APK）** — 合并 v1 麦克风权限 + overlay / 无障碍 manifest，用于真机 ADB 测试完整悬浮窗与无障碍能力（非仅应用内听写）：

```bash
cd openless-all/app
npm ci && npm run build
CI=true npm run tauri -- android init --ci
node scripts/copy-android-scaffolding.mjs
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
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
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
npm run tauri:android:build
```

## 相关文档

- [AGENTS.md](../../AGENTS.md) — 真机闪退排查
- [docs/android-mobile-apk-overlay-plan.md](../../docs/android-mobile-apk-overlay-plan.md) — 分阶段产品计划
