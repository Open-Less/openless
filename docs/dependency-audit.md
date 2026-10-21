# 依赖审计（全部工程）

审计时间：2026-10-19 · HEAD `feat/linux-egui-followups`

工程里有**四个独立的依赖图**，必须分别看（Rust 侧其实是**三个独立锁文件**：
根 workspace、`src-tauri`、`src-tauri/backend-tests`——三者都要单独 `cargo update`，
CI 一律用 `--locked`）：

| 依赖图 | 位置 | 谁在用 |
|---|---|---|
| A. 根 workspace | `openless-all/app/Cargo.toml`（成员：`crates/openless-core`、`linux-egui`、`harmony/rust`） | Linux 宿主、Core、鸿蒙 |
| B. Tauri workspace | `openless-all/app/src-tauri/`（独立 `Cargo.lock`，根 workspace 的 `exclude` 里有它） | Windows / macOS / Android 客户端 |
| B2. backend-tests workspace | `openless-all/app/src-tauri/backend-tests/`（又一个独立 `Cargo.lock`） | 远程输入 TLS 身份的集成测试 |
| C. npm | `openless-all/app/package.json` | React 前端 + 打包脚本 |
| D. Gradle/Kotlin | **不在仓库内**：由 `tauri android init` 生成，`scripts/copy-android-scaffolding.mjs` 只拷 Kotlin 源码并打补丁 | Android |

---

## A. 根 workspace —— 已在最新

- `cargo update` 把 42 个包推到最新兼容版本；直接依赖全部等于 crates.io 最新稳定版
  （校验方式：逐个 `cargo info <crate>` 对比）。
- egui/eframe 已升到 **0.37.0**（此前钉死 0.36.2），Core 的 `sha2` 升到 0.11.1。

仍落后但**本仓无法再升**：

| 包 | 当前 | 最新 | 卡在哪 |
|---|---|---|---|
| `base64` | 0.22.1 | 0.23.1 | axum 0.8.9 与 harmony 宿主依赖 0.22（本仓直接依赖已是 0.23.1） |
| `generic-array` | 0.14.7 | 0.14.9 | 被 crypto 依赖链的多条 `^0.14` 约束共同钉住 |
| `matchit` | 0.8.4 | 0.8.6 | axum 0.8.9 写死 `matchit = "=0.8.4"` |

未取的预发布版（不建议）：`libc 1.0.0-alpha.5`、`rustls 0.24.0-dev.2`。

## B. Tauri workspace —— 补丁级已升，主版本被 tauri 2.11 挡着

本轮已升（同一 train 的最新补丁）：`tauri 2.11.6`、`tauri-build 2.6.3`、
`tauri-runtime 2.11.3`、`tauri-runtime-wry 2.11.4`、`tauri-macros 2.6.3`、
`tauri-utils 2.9.3`；并 `cargo update` 刷新该独立锁文件。

**被 tauri 版本钉住、升不动**（尝试过，cargo 解析直接失败）：

| 目标 | 需要 | 冲突 |
|---|---|---|
| tauri 2.12.4 / tauri-build 2.7.3 / tauri-macros 2.7.3 / tauri-codegen 2.7.3 / tauri-utils 2.10.2 / tauri-runtime(-wry) 2.12.2 | `tauri-utils ^2.10` | `Cargo.toml` 里整条 train 用 `=` 钉死，注释记录了 2.12.0 时代的编译失败（Monitor / UnexpectedMenuKind） |
| tauri-plugin-fs 2.7.1 / dialog 2.8.3 / shell 2.5.0 / updater 2.14.0 / single-instance 2.5.4 / autostart 2.7.0 | `tauri-utils ^2.10` | 同上：`failed to select a version for tauri-utils ... required by tauri-plugin-fs v2.7.1` |

要越过这道墙，必须整体迁到 tauri 2.12 训练线并重测 Windows/macOS/Android 三端编译
（本地无法验证：`src-tauri` 已不支持 Linux 目标，`cargo check` 在 Linux 上因
`keyring` 等 target 依赖直接失败）。

**与 tauri 无关、可独立升级的主版本**（需要改代码 + 三端 CI 验证，本轮未动）：

axum 0.7→0.8、base64 0.22→0.23、bzip2 0.4→0.6、cpal 0.15→0.18、enigo 0.3→0.6、
getrandom 0.3→0.4、global-hotkey 0.6→0.9、hmac 0.12→0.13、jni 0.21→0.22、
keyring 3.6.3→4.2.0、md-5 0.10→0.11、minisign-verify 0.2→0.3、objc2 0.5→0.6
（连带 `objc2-foundation`/`objc2-app-kit` 0.2→0.3、`block2` 0.5→0.6、
`core-graphics` 0.24→0.25）、rcgen 0.13→0.14、reqwest 0.12→0.13、sha1 0.10→0.11、
sha2 0.10→0.11、thiserror 1→2、tokio-tungstenite 0.24→0.30、windows 0.58→0.62
（连带 `windows-core` 0.58→0.100）、winreg 0.52→0.56、window-vibrancy 0.7→0.8、
zip 2→9、x509-parser 0.16→0.18、toml 0.8.2→0.8.23（传递依赖，锁文件层面）。

**刻意钉死、不建议动**：

- `sherpa-onnx = 1.13.2`：Android arm64 用官方预编译库，版本必须与
  `scripts/sherpa-onnx-android.mjs` 校验的产物一致。
- `security-framework = 3.7.0`、`tao 0.35`（由 tauri-runtime-wry 约束）、
  `foundry-local-sdk`（winml 特性与 Windows 运行时绑定）。
- `tauri-nspanel`：来自 git 分支 `v2`，没有版本化发布。

## B2. 用 `=` 钉住的 15 处（钉住 ≠ 落后）

`Cargo.lock` 本来就把整个图锁定；这里的「钉」指 `Cargo.toml` 里的 `=x.y.z` 写法，
它会让 `cargo update` 连补丁版本都不再前进。分三类看：

**① 钉住且正好是最新（钉与不钉今天没区别，钉着防未来破坏性小版本）**

| 包 | 钉住 | 最新 |
|---|---|---|
| egui / eframe | 0.37.0 | 0.37.0 |
| resvg | 0.48.1 | 0.48.1 |
| tauri | 2.11.6 | 2.11.6（2.x 稳定线） |
| tauri-build / tauri-macros | 2.6.3 | 2.6.3 |
| tauri-runtime | 2.11.3 | 2.11.3 |
| tauri-runtime-wry | 2.11.4 | 2.11.4 |
| tauri-utils | 2.9.3 | 2.9.3 |
| security-framework | 3.7.0 | 3.7.0 |

**② 钉住且确实落后（有升级空间，但都受外部条件约束）**

| 包 | 钉住 | 最新 | 约束 |
|---|---|---|---|
| tauri-plugin-updater | 2.10.1 | 2.14.0 | 需要 `tauri-utils ^2.10`，即 tauri 2.12 训练线 |
| sherpa-onnx | 1.13.8 | 1.13.8 | 已升级；Android 归档的 size + SHA-256 在 `scripts/sherpa-onnx-android.mjs` 里同步（46_093_321 / `2ff63469…4f44`） |
| foundry-local-sdk | 1.2.1 | 2.1.0 | Windows winml 绑定，主版本跨越大 |

**③ 被上游依赖钉死，本仓改不了** —— 见上面 A / B 两节的表
（`base64 0.22.1`、`generic-array 0.14.7`、`matchit 0.8.4`、tauri 2.12 全线）。

## C. npm —— 次版本已升，剩 11 个主版本

`npm update` 升了 13 个（`@base-ui/react`、`dompurify`、`i18next`、`lucide-react`、
`react-i18next`、`shadcn`、`tailwind-merge`、`tailwindcss`/`@tailwindcss/vite`、
`@tauri-apps/api`、`@tauri-apps/cli`、`@tauri-apps/plugin-*` 等）。

另外 4 个原本写成精确版本的开发工具也一起升了：
`@playwright/test 1.63.0 → 1.64.1`、`prettier 3.9.6 → 3.9.10`、
`prettier-plugin-sh 0.19.0 → 0.20.2`、`tsx 4.23.1 → 4.23.15`。

`npm run build`（含 `tsc`）与 `node scripts/frontend-test-runner.mjs` 均通过。

仍为主版本的：

| 包 | 当前 | 最新 |
|---|---|---|
| react / react-dom | 18.3.1 | 19.3.1 |
| @types/react / @types/react-dom | 18.x | 19.3.0 |
| vite | 6.4.4 | 8.3.4 |
| @vitejs/plugin-react | 4.7.0 | 6.1.2 |
| typescript | 5.9.3 | 7.0.2 |
| framer-motion | 12.43.0 | 14.4.0 |
| marked | 11.2.0 | 18.1.1 |
| js-yaml | 4.3.0 | 5.5.0 |
| @shadcn/react | 0.2.1 | 0.3.1 |

React 19 / Vite 8 / TS 7 是一次前端大迁移，和 Linux egui 宿主无关，建议单独一轮做。

## D. Gradle/Kotlin

仓库里没有 `build.gradle.kts`。Android 工程由 `tauri android init` 生成，
`scripts/copy-android-scaffolding.mjs` 只负责拷 Kotlin 源码、改生成出来的
`build.gradle.kts`。因此 Gradle 依赖版本跟着 Tauri 的 Android 模板走 ——
只有升级 tauri 2.12（见 B）才会变化，本仓不能单独升。
