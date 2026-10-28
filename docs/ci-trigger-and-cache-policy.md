# CI 触发范围与缓存配额

状态：canonical；更新：2026-09-29。本文说明 CI 为什么在什么情况下跑哪些 job、缓存在 10 GB 预算内怎么分配，以及发版前怎么把耗时压下来。

## 1. 触发与门控

| 事件 | 行为 |
|---|---|
| `pull_request` → `main` / `beta` | 按改动范围门控：改动够不到的平台直接跳过（见下） |
| `push` → `main` / `beta` | 全量验证（合并后的基线必须有完整证据），不跳过任何 job；也是 Rust / Playwright 缓存的唯一写入方 |
| `push` → 其他分支 | **不跑**。feature 分支由它的 PR 覆盖；两边都跑等于同一个 commit 交两遍完整矩阵（实测 42 次 run 里有 8 次是这种重复，每次 ~86 VM-min、峰值 12 台 VM） |
| tag `v*-tauri` | 发布 Tauri 桌面与 Android；Linux 包不随标签自动发布 |
| `workflow_dispatch` | 全量；`platform=macos` 只跑 macOS（仅 macOS 验证入口） |

同一 PR 连续推送会取消旧运行（`concurrency.cancel-in-progress`）。

### 改动范围判定

`openless-all/app/scripts/ci-impact.mjs`（由 `ci-changed-areas.sh` 调用）回答一个问题：**这份改动能不能被某个平台的编译过程看见**。

Rust 部分不靠手写路径表，而是走 Cargo 自己的包图：`cargo metadata --no-deps --offline` 给出每个 workspace 包、它的目录、仓库内部依赖边，以及每条边上的 `cfg(...)` 守卫（例如 `openless → openless-capsule` 带 `cfg(not(any(target_os = "android", target_os = "ios")))`）。从每个平台的根包出发走一遍闭包，就得到该平台真正会编译的 crate 集合；一个文件在范围内，当且仅当**拥有它的包**在该集合里。非 Rust 的树（前端 bundle、ArkTS、Xcode 工程、工具链脚本）由脚本里的显式规则覆盖。

| 区域 | 覆盖的 job | 判定 |
|---|---|---|
| `tauri` | macOS / Windows 桌面检查 | 改动能到达 `openless`（含 vendor 路径依赖）或桌面专属的 `openless-native-capsule` |
| `android` | Android cargo check | 改动能到达 `openless` 的 Android 闭包，或影响 Android 项目生成/构建的 npm manifest；纯 bundle-only 改动不单独触发 |
| `msrv` | macOS Rust 1.99 MSRV | 有任何 `.rs` / `Cargo.toml` / `Cargo.lock` / `rust-toolchain*` / `.github/**` |
| `linux` | Linux Core/egui 测试与 deb/rpm 打包链 | 改动能到达 `openless-linux-egui` 的闭包。Linux 宿主是原生 egui，不读前端 `dist/` |
| `harmony` | 鸿蒙 ArkTS 契约、Rust host、HAP | 改动能到达 `openless-harmony-host` 的闭包，或动了它要打包的前端 bundle |
| `frontend` | Chromium/WebKit UI 动效 | 只动浏览器会渲染的面：`src/**`、`e2e/**`、样式与 vite/playwright 配置；`app/assets/**` / `app/public/**` 按 native consumer 处理 |
| `capsule` | 胶囊 crates（macOS + Windows 双 runner） | 拥有改动文件的包是 `openless-capsule` / `openless-native-capsule`。它们的 `ui,host` feature 组合不在别处构建，所以保留独立 job，但常见 PR 不再为它付出两台 runner |
| `ios-device` | iOS `device-build`（Xcode/Rime 与真实 device Core 编译） | 改动能到达 iOS Core 闭包，或动了 `openless-all/ios/**`；共享 Rust 代码也要保留 device target 证据 |

契约测试 `scripts/ci-changed-areas.test.mjs` 用真实 git diff 驱动这些判定，双向断言：够不到的必须跳过，其余的必须运行。改动前它抓到的真实故障（`linux` 输出漏写导致 Linux job 被误跳过）就是这条规则存在的理由。

`linux-egui-package` 通过可复用工作流的 `scope` 输入接收结果（`full` / `none`，**默认 `full`**）：纯散文改动跳过整个 Linux 构建与打包链。独立 Linux 手动构建沿用完整检查，只上传 Actions 产物；管理员在产品验收后，针对已接受的现有 tag 构建并单独附加包，见 [发布规范](../RELEASING.md)。

`ui-motion` 使用生产前端构建运行 `npm run test:ui-motion`，在 Chromium 与 WebKit 上验证快速往返切页、弹窗中断、手机抽屉退出以及减少动态效果下的模型入口规则。失败时保留浏览器 trace 和截图；浏览器 mock 验证不替代原生权限与设备交互。

## 2. MSRV 为什么是独立 job

- `openless-core`、`src-tauri`、`src-tauri/backend-tests`、`linux-egui` 统一声明 `rust-version = "1.99"`，README 对外承诺该版本，因此必须有 CI 用真的 1.99 编译：MSRV 只会被“依赖升级抬高要求”或“自己用了更新的语言特性/API”打破，两者都只有在 1.99 下编译才会暴露。
  该数字由依赖实际要求决定：`tauri 2.12` 要 1.90、`aes 0.9.3`/`uuid 1.28` 要 1.89，代码库选择跟随当前 stable（1.99）以保持单一数字，不再为旧工具链回退依赖。
- 工具链不同 ⇒ cargo fingerprint 全失效、产物与缓存不能共用。历史上 stable 与 MSRV 挤在同一个 job 里共用缓存，串行 29 分 59 秒（见 [`macos-build-performance.md`](macos-build-performance.md)）；拆成并行独立 job、缓存分开后是 stable 8m36s / MSRV 3m39s。
- 平台专属依赖图不同，所以 macOS 与 Windows 各自验证：macOS 是 `keyring/apple-native`、MLX/qwen-asr；Windows 是 `keyring/windows-native`、TSF IME。
- Linux 宿主 `linux-egui` 与桌面侧同用 `rust-version = "1.99"`（egui 0.37 本身只要求 1.95），Linux 侧按 stable 验证。

## 3. 缓存配额分配（10 GB / 仓库）

GitHub 每仓库保留 10 GB 缓存，超出按 LRU 静默淘汰；淘汰后 job 只会打印 `No cache found.`，表现为“无理由全量冷编译”。实测过的对照：`cargo test -p openless-core` 命中 148 s，冷启动 579 s。

实测体积与收益（一次命中节省 / 该 job 缓存体积）：

| job | 缓存 | 命中收益 | 性价比 |
|---|---|---|---|
| macOS stable | 0.87 GB | −11.4 min | 786 s/GB |
| Linux egui | 0.75 GB（+ 旧前缀 1.28 GB 待回收） | −10.8 min | 864 s/GB |
| macOS Rust 1.99 MSRV | 0.72 GB | −13.1 min | 1102 s/GB（与 stable 同号时整个 job 跳过，见 §2） |
| Android | 0.85 GB | −2.4 min | 169 s/GB |
| Windows | 1.57 GB | −4.6 min | 176 s/GB |
| Harmony Rust host | 0.28 GB | −2–4 min | ~600 s/GB |
| Android Rime NDK | 0.40 GB | −3 min | 450 s/GB |
| Playwright 浏览器 | 0.37 GB | −1.5 min | 243 s/GB |

**不缓存鸿蒙命令行工具链**：解压后 7.1 GB，缓存条目 1.77 GB，而下载 2.1 GB 只要 ~18 s（108 MB/s）、解压 ~70 s。恢复缓存本身就要几十秒，还要占掉 18% 的配额，性价比为负。`prepare-harmony-sdk.sh` 按 SHA-256 固定校验和，每次重下反而是更可靠的选择。

分配原则：**按平台配额 + 只有基线分支/tag/迭代中的 job 允许写。**

| 归属 | 目标 | 谁写 | 谁读 |
|---|---|---|---|
| Windows 依赖树 | ~1.6 GB | `push`（beta）、tag | 所有 PR、发版 |
| macOS（stable 0.87 + MSRV 0.72） | ~1.6 GB | 同上 | 同上 |
| Linux（egui 0.75 + core） | ~1.7 GB | `push`、tag，**以及 PR 里的 `build-linux-egui`** | 同上 |
| Android（Rust 0.85 + Rime 0.40 + gradle 0.31） | ~1.6 GB | `push`、tag | PR |
| Harmony Rust host | 0.28 GB | `push`、tag | PR |
| Playwright 浏览器 | 0.37 GB | `push`、tag | PR |
| 鸿蒙工具链 | **0（不缓存）** | — | — |
| 发版（tag 作用域） | ~1.0 GB | tag / 发版前预热 | — |
| 余量 | ≥1.5 GB | — | — |

对应的机制：

- 重量级 job 用 `save-if: ${{ github.ref == 'refs/heads/beta' && (github.event_name == 'push' || github.event_name == 'workflow_dispatch') }}`：PR 只从基线分支恢复，不再各自复制一份 1.6 GB 依赖树（此前两个 PR 就占了 10.87 GB，把基线缓存挤掉）。
- **禁止 `uses: actions/cache@v4`**：它会在每次运行时自动保存，包括 `refs/pull/<n>/merge` 这种此后再也不会被恢复的作用域。鸿蒙工具链（1.77 GB）和 Rime NDK（0.40 GB）都这样泄漏过——两个 PR 推送就能把 beta 的依赖树挤掉。规则统一成 `actions/cache/restore@v4` 无条件恢复 + 独立的 `actions/cache/save@v4` 且 `if:` 限定 beta。
- `playwright` 与 `mobile-rime` 使用 restore/save 拆分；`swatinem/rust-cache` 用 `save-if` 达到同样效果。
- `build-linux-egui` 保留 PR 内自写并开 `cache-on-failure: true`：失败的运行也会把依赖缓存存下来，下一次迭代仍然热；同时关掉 dev/test debug info，缓存更小、链接更快。
- key 结构为 `v0-rust-<job>-<os>-<环境哈希>-<lockfile哈希>`：环境哈希含工具链与 `CARGO_PROFILE_*`，所以 stable / MSRV / release 天然分开，不要用跨平台 `shared-key` 混合。
- 原生胶囊（`native-capsule` job）原本是一个 `pull_request`-only workflow：缓存写在 `refs/pull/<n>/merge`，这个作用域此后再不会被恢复，而 beta 上没有 job 产出同 key 的树，所以 PR 永远冷编译。现在它在 `ci.yml` 里，用 `shared-key: native-capsule` + beta 门控的 `save-if`，并且由 `capsule` 区域门控——常见 PR 完全不会为它拉起 runner。
  曾试过把它并进 `cross-platform`（省两台 VM），实测否决：run 40510306688 里那三个命令在 Windows 关键路径上花了 43+53+21 s，比它省下的两台 runner 更贵。要合并先要确认那套 `ui,host` feature 在桌面 job 的缓存里是热的。
- `Cargo.lock` 变化时精确键失效，但 `restore-keys` 前缀回退会拿到同前缀的旧缓存，所以“锁变了”不等于“全冷”——前提是那份没被淘汰，这正是要守预算的原因。

## 4. 维护

- 报告与清理：`bash openless-all/app/scripts/ci-cache-usage.sh [--prune] [--limit-gb 8] [--repo OWNER/NAME]`。`--prune` 只删已关闭/已合并 PR 的 `refs/pull/*` 作用域（先查 PR 状态，状态读不到就保留），永不触碰分支与 tag 作用域；用量超过预算时退出码为 1。
- 自动维护：`.github/workflows/cache-maintenance.yml` 每周一 03:17 UTC 运行并支持手动触发，预算 8 GB。
- 契约测试 `scripts/ci-cache-sharing.test.mjs` 会拒绝任何 `uses: actions/cache@v4`（自动保存）以及任何鸿蒙工具链缓存键，防止上面两条规则被无意改回去。
- 合并/关闭 PR 后可手动立即回收：`gh api -X DELETE repos/Open-Less/openless/actions/caches?ref=refs/pull/<N>/merge`。

## 5. 实测对照

同一 PR（#1060）、同一批 job，改动前后：

| job | 改动前（冷 / 超配额） | 缓存配额生效后 | 门控生效后 |
|---|---|---|---|
| Changed areas | — | — | 11 s |
| macOS checks | 20m01s | 8m36s | 6m16s |
| Windows checks | 20m04s | 15m30s | 13m17s |
| Android | 8m11s | 5m50s | 5m50s |
| macOS Rust 1.99 MSRV | 16m47s | 3m39s | 3m39s |
| Linux egui（测试 + 打包） | 17m21s | 6m35s（旧缓存前缀） | 8m08s（新前缀首次命中） |

同一步骤的冷 / 热对照：`cargo test -p openless-core` 命中 201 s、冷启动 579 s；`cargo build --release -p openless-linux-egui` 命中 124 s、冷启动 463 s。整轮墙钟时间由最慢的 job 决定（本轮 Windows 13m17s），改动前是 20m12s。

## 6. 发版耗时

`Release Tauri (cross-platform)` 的基线是 ~32.7 分钟，关键路径是 macOS Intel 构建。原因是 tag 运行只能读默认分支（`beta`）的缓存，而基线分支跑的是 debug/test profile，不产生 `macos-release-v1` 前缀的 release 缓存，因此每个新 tag 都要从零编译 release 依赖。

**发版前先预热**：把一个 `Release Tauri (cross-platform)` 运行 dispatch 到普通分支（上游历史上就在 `perf/macos-ci-build` 这样做过，单次 11–19 分钟），预热完成后再打 tag，可把发版压到 ~10 分钟量级。

Linux 发版不受影响：`release-linux-egui.yml` 复用 CI 的同一个 `build-linux-egui` job（同名同 key），tag 运行能恢复基线分支的缓存。

## 7. 已知取舍

- 改动范围门控只跳过“够不到”的平台；`push` 到 `main`/`beta` 始终全量，所以合并后仍有完整证据。
- `build-linux-egui` 在 `push` 时仍会构建 release 二进制（保持基线缓存里有 release 产物、并持续验证打包链），但不再上传 Action artifact；artifact 只在 PR 与 tag 运行产生。
- 所有 job 都设了 `timeout-minutes`，避免卡死的运行长时间占用并发与时间。
