# OpenLess Android APK 与悬浮窗实施计划

> 状态：实施中 / 后端分层已落地
> 日期：2026-06-07
> 范围：Android APK v1（应用内录音）→ IME v2（跨 App 输入）→ 悬浮窗 v3；不改桌面语义

目标是把现有 OpenLess 桌面应用扩展为 Android APK，并在手机端提供可用的语音输入体验。当前项目是 React/Vite + Tauri v2 + Rust 后端，Tauri 官方支持 Android 构建；现仓库已增加 Android 分层与脚手架，桌面核心能力（全局热键、托盘、桌面浮窗、TSF IME 等）通过 `#[cfg(not(mobile))]` 收口。

核心策略：先产出可安装 APK，再做 Android 输入法服务，最后做跨 App 悬浮窗。桌面功能保持不受影响。

参考依据：
- Tauri Android 依赖：Android Studio、SDK/NDK、`ANDROID_HOME`、`NDK_HOME`、Android Rust targets。
- Tauri Android 构建命令：`tauri android init`，`tauri android build --apk`。
- Android 悬浮窗：Android 8.0+ 使用 `SYSTEM_ALERT_WINDOW` + `TYPE_APPLICATION_OVERLAY`。

---

## 1. 目标与非目标

### 目标

- **APK v1**：应用内主窗口、设置、历史、云端 ASR/LLM、基础录音、复制结果
- **IME v2**：`OpenLessImeService` 作为跨 App 输入主路径
- **悬浮窗 v3**：前台服务 + `TYPE_APPLICATION_OVERLAY`，仅作录音控制入口
- **平台能力查询**：前端通过 `get_platform_capabilities` 隐藏桌面专属设置项
- **桌面零破坏**：所有适配经 `#[cfg(not(mobile))]` / `#[cfg(mobile)]` 分层

### 非目标

- APK 首版不纳入本地 ASR（Foundry、Sherpa、Qwen 桌面假设）
- 首版不承诺直接写入其他 App（无 IME 时走复制兜底）
- Accessibility 跨 App 输入不作为默认路径
- 悬浮窗不承担最终文本插入职责
- 不改桌面现有功能语义

### 明确边界

- **不动** macOS / Windows / Linux 热键、托盘、胶囊、QA 浮窗逻辑
- **Android 命令名与桌面一致**；不支持的命令返回明确 unavailable 状态
- **Coordinator 听写主链路复用**；`end_session` 在 Android 增加 IME commit 分支

---

## 2. 架构定位

```
openless-all/app/
  package.json              # tauri:android:* scripts
  vite.config.ts            # TAURI_ENV_PLATFORM → 0.0.0.0 + HMR
  src-tauri/
    tauri.conf.json         # bundle.android.minSdkVersion ≥ 26
    tauri.android.conf.json # 单 main 窗口（无 capsule/qa/tray）
    capabilities/
      default.json          # 桌面
      mobile.json           # Android 主窗口
    android-scaffolding/    # Kotlin 模板（init 后复制到 gen/android）
    src/
      lib.rs                # desktop/mobile run() 分层
      android_ime.rs        # IME 状态 + commit（JNI 桩）
      android_overlay.rs    # 悬浮窗权限/状态（JNI 桩）
      permissions.rs        # Android 麦克风 runtime permission 分支
      persistence.rs        # Android 凭据加密文件桩
      types.rs              # PlatformCapabilities
      commands.rs           # get_platform_capabilities + 桌面命令 stub
      coordinator/dictation.rs  # end_session Android IME 分支
```

平台分层示意：

```
┌─────────────────────────────────────────────────────────┐
│  React UI（能力查询 → 隐藏桌面专属设置）                  │
├─────────────────────────────────────────────────────────┤
│  Tauri commands（同名 IPC；mobile 返回 unavailable stub） │
├──────────────┬──────────────────────────────────────────┤
│  desktop     │  mobile (Android)                         │
│  hotkey/tray │  in-app dictation + cloud ASR           │
│  capsule/qa  │  android_ime (v2) / android_overlay(v3) │
│  TSF/AX/粘贴 │  IME commit (v1); clipboard TBD        │
└──────────────┴──────────────────────────────────────────┘
```

> **v1 剪贴板**：APK v1 不使用 Android 剪贴板兜底（未接 arboard）；跨 App 文本输入依赖后续 IME/JNI 接线。

---

## 3. 模块设计

### 3.1 `PlatformCapabilities`（`types.rs`）

```rust
pub struct PlatformCapabilities {
    pub platform: String,
    pub supports_ime_input: bool,
    pub supports_overlay: bool,
    pub supports_desktop_hotkey: bool,
    pub supports_tray: bool,
    pub supports_local_asr: bool,
    pub supports_capsule_overlay: bool,
}
```

- Android：`supportsImeInput=true`（v2 起）、`supportsOverlay=true`（v3 起）、`supportsDesktopHotkey=false`、`supportsTray=false`
- 桌面：按 OS 填真实能力

### 3.2 `android_ime.rs`

- `get_android_ime_status()` → 是否已启用 OpenLess 输入法
- `commit_text(text)` → 通过 JNI 提交到 `InputConnection`（桩 → 日志 + 返回未连接）
- Kotlin：`OpenLessImeService` 继承 `InputMethodService`

### 3.3 `android_overlay.rs`

- `get_android_overlay_status()` → `SYSTEM_ALERT_WINDOW` 授权状态
- `request_android_overlay_permission()` → 跳转 `OverlayPermissionActivity`
- Kotlin：`OpenLessOverlayService`（前台服务 + overlay window）

### 3.4 `permissions.rs` / `persistence.rs`

- 麦克风：Android runtime permission 分支（JNI 桩；未接线时 `NotDetermined`）
- 凭据：Android 加密 JSON 文件桩（`credentials.enc.json`），不沿用桌面 keyring

### 3.5 `coordinator/dictation.rs`

`end_session` 插入阶段：

```
#[cfg(target_os = "android")]
  android_ime::commit_text → Inserted
  失败 → copy_fallback → CopiedFallback
#[cfg(not(mobile))]
  现有 Windows TSF / AX / paste 路径
```

---

## 4. 原生层接口

| 组件 | 职责 |
|---|---|
| `MainActivity` | Tauri WebView + 权限状态桥接 |
| `OpenLessImeService` | 接收识别结果，`commitText` 到当前输入框 |
| `OpenLessOverlayService` | 悬浮窗开始/停止录音、显示状态 |
| `OverlayPermissionActivity` | 引导用户授权 `SYSTEM_ALERT_WINDOW` |

Rust ↔ Kotlin 通信：Tauri mobile plugin / `jni`（脚手架阶段为桩，init 后接线）。

---

## 5. 实施里程碑

### M0 环境与文档（本期）

- 扩展本计划文档（元数据、架构、文件表、风险）
- `package.json`：`tauri:android:init|dev|build`
- `vite.config.ts`：mobile dev host `0.0.0.0` + HMR
- `tauri.conf.json`：`bundle.android.minSdkVersion: 26`
- `tauri.android.conf.json` + `capabilities/mobile.json`

### M1 Rust 分层 + APK 骨架（本期）

- `lib.rs` desktop/mobile `run()` 分层
- `Cargo.toml` gate 桌面专属依赖
- `PlatformCapabilities` + 命令 stub
- `permissions.rs` / `persistence.rs` Android 分支
- `cargo check` 桌面通过；Android target 尽力验证

### M2 IME v2

- 接线 `OpenLessImeService` JNI
- 设置页显示输入法启用状态
- 跨 App 提交验收

### M3 悬浮窗 v3

- 接线 `OpenLessOverlayService`
- 授权引导 + 前台服务稳定性

---

## 6. 文件触达表

| 文件 | 变更 |
|---|---|
| `docs/android-mobile-apk-overlay-plan.md` | 扩展为完整实施规划（本文档） |
| `openless-all/app/package.json` | `tauri:android:*` scripts |
| `.github/workflows/android-apk.yml` | Android debug APK CI |
| `openless-all/app/scripts/merge-android-v1-manifest.mjs` | v1 manifest merge (RECORD_AUDIO) |
| `openless-all/app/vite.config.ts` | mobile dev server / HMR |
| `openless-all/app/src-tauri/tauri.conf.json` | `bundle.android` |
| `openless-all/app/src-tauri/tauri.android.conf.json` | 单 main 窗口 |
| `openless-all/app/src-tauri/capabilities/mobile.json` | Android 权限集 |
| `openless-all/app/src-tauri/Cargo.toml` | gate desktop deps |
| `openless-all/app/src-tauri/src/lib.rs` | mobile/desktop 分层 |
| `openless-all/app/src-tauri/src/types.rs` | `PlatformCapabilities` 等 |
| `openless-all/app/src-tauri/src/commands.rs` | 能力查询 + mobile stub |
| `openless-all/app/src-tauri/src/permissions.rs` | Android 麦克风 |
| `openless-all/app/src-tauri/src/persistence.rs` | Android 凭据 |
| `openless-all/app/src-tauri/src/android_ime.rs` | 新增 |
| `openless-all/app/src-tauri/src/android_overlay.rs` | 新增 |
| `openless-all/app/src-tauri/src/coordinator/dictation.rs` | `end_session` Android 分支 |
| `openless-all/app/src-tauri/android-scaffolding/*.kt` | Kotlin 模板 |

---

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| 本机无 Android SDK / NDK | 文档记录手动脚手架；`android-scaffolding/` 供 init 后复制 |
| `global-hotkey` / `enigo` / `arboard` 无法编 Android | `Cargo.toml` `cfg(not(mobile))` gate |
| `keyring` 在 Android 不可用 | 加密文件桩 + 后续 Keystore 接线 |
| 桌面构建被 mobile 分层破坏 | 桌面 `cargo check` 为 CI 门禁；mobile 代码 `#[cfg(mobile)]` 隔离 |
| JNI 未接线时 IME/overlay 假成功 | 状态查询返回 `enabled=false`；commit 走 copy fallback |
| 多窗口配置污染移动端 | `tauri.android.conf.json` 仅声明 `main` |
| 本地 ASR 拖慢 Android 首版 | 明确排除；`supportsLocalAsr=false` |
| `SYSTEM_ALERT_WINDOW` 用户拒绝 | 设置页显示状态；悬浮窗功能降级为应用内入口 |

---

## Android APK CI Workflow

GitHub Actions workflow: [`.github/workflows/android-apk.yml`](../.github/workflows/android-apk.yml)

### Capability platform isolation

Desktop permissions live in `capabilities/default.json` with `"platforms": ["macOS", "windows", "linux"]`, so updater, autostart, and multi-window permissions do not apply on Android. Android uses `capabilities/mobile.json` with `"platforms": ["android"]` for the main-window permission set. In-app updates use a custom Rust updater (`src-tauri/src/android/updater.rs`) because `tauri-plugin-updater` does not support Android.

### Triggers & channels

| Trigger | Build mode | Behavior |
|---|---|---|
| `workflow_dispatch` | **release** if `ANDROID_KEYSTORE_*` secrets configured; else **debug (unsigned)** | Upload Actions artifacts; non-blocking fallback with job summary notice when unsigned |
| Push tag `v*-tauri` / `v*-beta-tauri` | **release** (required secrets) | Signed release APKs + minisign `.sig` + `latest-android-{arch}[-beta].json` → attach to GitHub Release |

`OPENLESS_RELEASE_CHANNEL` matches desktop `release-tauri.yml`: `-beta-tauri` → beta (prerelease manifests); otherwise stable.

Tag releases require secrets: `TAURI_SIGNING_PRIVATE_KEY` (minisign), `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` (APK signing).

### APK naming

| ABI | Release (tag) | Debug (dispatch) |
|---|---|---|
| arm64-v8a | `OpenLess_<version>_arm64-v8a.apk` | `OpenLess-android-debug-arm64-v8a-run-<n>.apk` |
| armeabi-v7a | `OpenLess_<version>_armeabi-v7a.apk` | `OpenLess-android-debug-armeabi-v7a-run-<n>.apk` |
| x86 | `OpenLess_<version>_x86.apk` | `OpenLess-android-debug-x86-run-<n>.apk` |
| x86_64 | `OpenLess_<version>_x86_64.apk` | `OpenLess-android-debug-x86_64-run-<n>.apk` |

Actions artifact names: `openless-android-release-{abi}` (tag) or `openless-android-debug-{abi}` (dispatch).

Updater manifests: `latest-android-aarch64.json` (arm64), `latest-android-armv7.json`, `latest-android-i686.json`, `latest-android-x86_64.json` (+ `-beta` / `-mirror` variants).

### Command chain (CI)

```bash
cd openless-all/app
npm ci && npm run build
npm run tauri -- android init --ci
node scripts/copy-android-scaffolding.mjs
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-updater-manifest.mjs
# tag only:
node scripts/configure-android-release-signing.mjs
npm run tauri:android:build:debug    # dispatch
npm run tauri:android:build:release  # tag
# tag only:
node scripts/sign-android-apks.mjs <apks...>
OPENLESS_UPDATE_APK_DIR=... OPENLESS_UPDATE_TARGET=android OPENLESS_UPDATE_ARCH=aarch64 node scripts/write-updater-manifest.mjs
```

`ci.yml` also runs `cargo check --target aarch64-linux-android` as a lightweight mobile cfg gate.

### Manifest merge

- `merge-android-v1-manifest.mjs` — `RECORD_AUDIO` (v1 in-app dictation)
- `merge-android-overlay-manifest.mjs` — overlay + accessibility service
- `merge-android-updater-manifest.mjs` — `REQUEST_INSTALL_PACKAGES` + `FileProvider` for APK install

### In-app updater (Android)

Custom Rust module [`openless-all/app/src-tauri/src/android/updater.rs`](../openless-all/app/src-tauri/src/android/updater.rs) + shared helpers [`updater_logic.rs`](../openless-all/app/src-tauri/src/android/updater_logic.rs). Desktop continues to use `tauri-plugin-updater`.

**Manifest URLs** (generated by [`write-updater-manifest.mjs`](../openless-all/app/scripts/write-updater-manifest.mjs)):

| Channel | Filename | GitHub path |
|---|---|---|
| Stable | `latest-android-{arch}.json` | `releases/latest/download/...` |
| Beta | `latest-android-{arch}-beta.json` | `releases/download/{v*-beta-tauri tag}/...` |

Client tries mirror URL first (`-mirror.json`), then direct GitHub. Beta tag is resolved from `releases.atom` (first `-beta-tauri` entry).

**User-facing behavior**:

- **Settings → About**: manual “Check stable update” always fetches stable manifest.
- **Settings → Advanced**: Beta toggle sets `prefs.updateChannel` for background checks; “Check Beta update” always fetches beta manifest (independent of toggle).
- **Settings → Advanced → Auto-update** (Android): `autoUpdateCheck` toggle — when on, `AutoUpdateGate` checks on launch (+4s) and every 60 minutes using `updateChannel`, then **automatically downloads**, minisign-verifies, and opens the **system APK installer**. When off, only manual buttons run.
- **Desktop**: `autoUpdateCheck` only auto-checks; user confirms in `UpdateDialog` before download/install/restart.

**Install flow**: download to app cache → minisign verify → JNI `install_apk_from_path` → Android system installer. Over-the-air replace is **not** implemented in-app; the user completes install via the system UI. Requires matching APK signature for upgrade.

**Prefs**: `updateChannel` (`stable` | `beta`) = background auto-update channel only; manual buttons pass explicit channel and ignore this pref.

---

## 8. 验收标准

### 构建验证

```bash
cd openless-all/app
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
# 需 Android SDK / NDK（与 CI 一致：debug APK）：
npm run tauri:android:init
npm run merge:android-v1-manifest
npm run tauri:android:build
# 等价于 CI 的 debug 构建：
# CI=true npm run tauri -- android init --ci && node scripts/merge-android-v1-manifest.mjs && CI=true npm run tauri:android:build
```

### APK v1

- 首次启动进入主界面
- 麦克风授权流程可触发
- 应用内录音 → 云端转写 → 历史 + 复制
- 桌面专属命令不导致前端白屏（返回 unavailable）
- 桌面 `cargo check` 仍通过

### IME v2 / 悬浮窗 v3

见原 Summary 中 Test Plan 章节（启用输入法后跨 App 提交；悬浮窗授权与前台服务稳定性）。

---

## Compatibility fixes（2026-06-07）

- **`app_invoke_handler_mobile`**：仅保留 dictation / settings / credentials / history / cloud ASR / platform capabilities / Android IME·overlay / permissions / marketplace / style packs / mic devices；已移除 `get_hotkey_*`、`set_shortcut_recording_active` 及全部 desktop-only 命令（热键 setter、updater、local ASR、coding agent、tray 等）。前端 `ipc.ts` 在 `supportsDesktopHotkey === false` 时本地返回 stub，不再 invoke 这些命令。
- **`mobile_stubs`**：`unicode_keystroke` 补齐 `typed_chars()` / `Partial`（与 coordinator 流式插入一致）；`shortcut_binding::binding_from_legacy_trigger` 与桌面实现对齐。
- **`Cargo.toml`**：`enigo` / `global-hotkey` / updater / single-instance / autostart 仅在 `cfg(not(mobile))`；Android 侧 `jni` + `ndk-context` 已声明。

---

## 9. 相关参考

- Tauri Android：https://v2.tauri.app/develop/mobile/
- 桌面 Windows ASR 规划风格：`docs/windows-sherpa-onnx-asr-plan.md`
- 主听写链路：`openless-all/app/src-tauri/src/coordinator/dictation.rs`
- Windows IME unavailable 模式：`openless-all/app/src-tauri/src/windows_ime_profile.rs`
