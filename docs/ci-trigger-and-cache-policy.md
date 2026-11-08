# CI 触发范围与缓存配额

状态：canonical；更新：2026-11-08。本文说明 CI 为什么在什么情况下跑哪些 job、缓存在 10 GB 预算内怎么分配，以及发版前怎么把耗时压下来。

## 1. 触发与门控

| 事件 | 行为 |
|---|---|
| `pull_request` → `main` / `beta` | 按改动范围门控：改动够不到的平台直接跳过（见下） |
| `push` → `main` / `beta` | 全量验证（合并后的基线必须有完整证据），不跳过任何 job；仅 beta 写入共享缓存，beta 手动/周期构建也可预热 |
| `push` → 其他分支 | **不跑**。feature 分支由它的 PR 覆盖；两边都跑等于同一个 commit 交两遍完整矩阵（实测 42 次 run 里有 8 次是这种重复，每次 ~86 VM-min、峰值 12 台 VM） |
| tag `v*-tauri` | 发布 Tauri 桌面与 Android；Linux 包不随标签自动发布 |
| `workflow_dispatch` | 全量；`platform=macos` 只跑 macOS（仅 macOS 验证入口） |

同一 PR 连续推送会取消旧运行（`concurrency.cancel-in-progress`）。CI 的相同 ref、platform、macos_runner 手动请求也会取消旧运行，不同参数独立运行。桌面手动构建按 ref、platform、诊断开关区分；Android 按 ref、ABI、profile、签名模式区分。标签发布（包括手动选择标签）和修复发布不会被新运行中途取消。

### 改动范围判定

`openless-all/app/scripts/ci-impact.mjs`（由 `ci-changed-areas.sh` 调用）回答一个问题：**这份改动能不能被某个平台的编译过程看见**。

Rust 部分不靠手写路径表，而是走 Cargo 自己的包图：`cargo metadata --no-deps --offline` 给出每个 workspace 包、它的目录、仓库内部依赖边，以及每条边上的 `cfg(...)` 守卫（例如 `openless → openless-capsule` 带 `cfg(not(any(target_os = "android", target_os = "ios")))`）。从每个平台的根包出发走一遍闭包，就得到该平台真正会编译的 crate 集合；一个文件在范围内，当且仅当**拥有它的包**在该集合里。非 Rust 的树（前端 bundle、ArkTS、Xcode 工程、工具链脚本）由脚本里的显式规则覆盖。

| 区域 | 覆盖的 job | 判定 |
|---|---|---|
| `tauri` | macOS / Windows 桌面检查 | 改动能到达 `openless`（含 vendor 路径依赖）或桌面专属的 `openless-native-capsule` |
| `android` | Android cargo check | 改动能到达 `openless` 的 Android 闭包，或影响 Android 项目生成/构建的 npm manifest；纯 bundle-only 改动不单独触发 |
| `msrv` | macOS / Windows Rust 1.99 MSRV | 有任何 `.rs` / `Cargo.toml` / `Cargo.lock` / `rust-toolchain*` / `.github/**` |
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
- 平台专属依赖图不同，所以 macOS 与 Windows 各自验证：macOS 是 `keyring/apple-native`；Windows 是 `keyring/windows-native`、TSF IME。Windows MSRV 也在独立 job 中运行，避免 stable → MSRV 串行重编译和污染 stable 缓存。`Windows checks` 汇总两路结果，任一路失败、取消或意外跳过都会失败。
- Linux 宿主 `linux-egui` 与桌面侧同用 `rust-version = "1.99"`（egui 0.37 本身只要求 1.95），Linux 侧按 stable 验证。

## 3. 缓存配额分配（10 GB / 仓库）

GitHub 每仓库保留 10 GB 缓存，超出按 LRU 静默淘汰；淘汰后 job 只会打印 `No cache found.`，表现为“无理由全量冷编译”。实测过的对照：`cargo test -p openless-core` 命中 148 s，冷启动 579 s。

实测体积与收益（一次命中节省 / 该 job 缓存体积）：

| job | 缓存 | 命中收益 | 性价比 |
|---|---|---|---|
| macOS stable | 0.87 GB | −11.4 min | 786 s/GB |
| Linux egui | 0.75 GB（+ 旧前缀 1.28 GB 待回收） | −10.8 min | 864 s/GB |
| macOS Rust 1.99 MSRV | 0.72 GB | −13.1 min | 1102 s/GB（即使 stable 同号也运行 pinned compiler，见 §2） |
| Android | 0.85 GB | −2.4 min | 169 s/GB |
| Windows | 1.57 GB | −4.6 min | 176 s/GB |
| Harmony Rust host | 0.28 GB | −2–4 min | ~600 s/GB |
| Android Rime NDK | 0.40 GB | −3 min | 450 s/GB |
| Playwright 浏览器 | 0.37 GB | −1.5 min | 243 s/GB |

**不缓存鸿蒙命令行工具链**：解压后 7.1 GB，缓存条目 1.77 GB，而下载 2.1 GB 只要 ~18 s（108 MB/s）、解压 ~70 s。恢复缓存本身就要几十秒，还要占掉 18% 的配额，性价比为负。`prepare-harmony-sdk.sh` 按 SHA-256 固定校验和，每次重下反而是更可靠的选择。

分配原则：**按平台配额，仅基线分支的 CI / 手动 / 周期构建写入共享缓存，PR 与 tag 只读。**

| 归属 | 目标 | 谁写 | 谁读 |
|---|---|---|---|
| Windows 依赖树 | ~1.6 GB | beta 的 CI / 手动构建 / 周期构建 | 所有 PR、发版 |
| macOS（stable 0.87 + MSRV 0.72） | ~1.6 GB | 同上 | 同上 |
| Linux（egui 0.75 + core） | ~1.7 GB | beta `push` / 手动运行 | 同上 |
| Android（Rust 0.85 + Rime 0.40 + gradle 0.31） | ~1.6 GB | beta 的 CI / 手动构建 / 周期构建 | PR、发版 |
| Harmony Rust host | 0.28 GB | beta `push` / 手动运行 | PR、发版 |
| Playwright 浏览器 | 0.37 GB | beta `push` / 手动运行 | PR |
| 鸿蒙工具链 | **0（不缓存）** | — | — |
| 发版（tag 作用域） | 不再新增 | — | tag（从 beta 恢复） |
| 余量 | ≥1.5 GB | — | — |

对应的机制：

- 重量级 job 用 `save-if: ${{ github.ref == 'refs/heads/beta' && (github.event_name == 'push' || github.event_name == 'workflow_dispatch') }}`：PR 只从基线分支恢复，不再各自复制一份 1.6 GB 依赖树（此前两个 PR 就占了 10.87 GB，把基线缓存挤掉）。
- **禁止 `uses: actions/cache@v4`**：它会在每次运行时自动保存，包括 `refs/pull/<n>/merge` 这种此后再也不会被恢复的作用域。鸿蒙工具链（1.77 GB）和 Rime NDK（0.40 GB）都这样泄漏过——两个 PR 推送就能把 beta 的依赖树挤掉。规则统一成 `actions/cache/restore@v4` 无条件恢复 + 独立的 `actions/cache/save@v4` 且 `if:` 限定 beta。
- `playwright` 与 `mobile-rime` 使用 restore/save 拆分；`swatinem/rust-cache` 用 `save-if` 达到同样效果。Playwright 按引擎与版本分键，每个 runner 只下载、恢复自己的浏览器；新键首次使用需要冷下载。
- npm 复合 action 内允许 beta 门控的 `actions/cache@v4`，其他 ref 只用 `cache/restore`；周期构建也可以维护 beta 下载缓存。
- 历史标签修复入口是兼容例外：标签可能早于 npm 复合 action，继续使用原有 `setup-node` npm 缓存；常规 tag 发布使用只读复合 action。
- Linux 测试与打包各自使用独立缓存，PR 只读、beta 写入；dev/test 关闭 debug info。Windows MSRV 新增独立缓存族，实际体积需在 beta 首次写入后由维护作业重新核算，不能把上表历史余量视为保证。
- key 结构为 `v0-rust-<job>-<os>-<环境哈希>-<lockfile哈希>`：环境哈希含工具链与 `CARGO_PROFILE_*`，所以 stable / MSRV / release 天然分开，不要用跨平台 `shared-key` 混合。
- 相同 `shared-key` 只是必要条件：恢复仍要求工具链/环境哈希和缓存路径版本兼容。CI 的 dev/test 缓存不能保证让 release 热启动，发布配置依赖 beta 的周期构建预热；不能把共同前缀当作已命中的证据。
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

**发版前预热必须在默认分支 `beta` 上执行**：其他分支的缓存不能被未来的 tag 恢复。桌面每周 schedule 在 beta 维护 release 缓存；手动预热也选择 beta。Android 周期构建默认只维护 aarch64；需要全部 release ABI 时在 beta 手动选择 `abis=all`。Tag 和其他分支只读 Rust/Gradle/npm 缓存，避免一次性副本挤掉可复用的基线。缓存命中仍需编译应用本身，不承诺固定发版时长。

Linux 发版不受影响：`release-linux-egui.yml` 复用 CI 的同一个 `build-linux-egui` job（同名同 key），tag 运行能恢复基线分支的缓存。

## 7. 已知取舍

- 改动范围门控只跳过“够不到”的平台；`push` 到 `main`/`beta` 始终全量，所以合并后仍有完整证据。
- `build-linux-egui` 在 `push` 时仍会构建 release 二进制（保持基线缓存里有 release 产物、并持续验证打包链），但不再上传 Action artifact；artifact 只在 PR 与 tag 运行产生。
- 所有 job 都设了 `timeout-minutes`，避免卡死的运行长时间占用并发与时间。

## 桌面整理后的依赖与维护

- 前端的 shadcn CSS 放在 `src/components/chat/vendor/`，MIT 许可通过 `public/licenses/shadcn-LICENSE.txt` 随前端产物分发，不再为构建一份样式安装 shadcn CLI 及其依赖。`package-lock.json` 从 503 个包记录减到 246 个；运行期组件包保持锁定。
- `cache-npm` 优先使用已缓存的下载，关闭 CI 安装时的 audit/fund 请求；单次网络请求上限 60 秒、重试两次。缓存分段恢复上限 3 分钟，避免异常下载长期占住 runner。
- 缓存清理每日运行并在 PR 关闭时触发；删除后重新读取实际用量，避免以旧用量错误报告仍超额。保留开放 PR、分支的可用代际及 tag 缓存的既有边界。
- 本地验证与远端耗时收益分别验收；依赖记录数量减少不等于某个平台编译时间已按比例下降。

- Chromium 与 WebKit 各在独立 runner 上运行，保持每个浏览器一个 worker；两边完成后汇总为原有 `UI motion (Chromium + WebKit)` 门禁。失败、取消或应运行却跳过均不能报告成功。失败截图和 trace 按浏览器分别保存。
- 手动桌面打包支持 `macos-aarch64` 与 `macos-x86_64`，可只构建验收所需架构。正式标签仍构建双 Mac 架构与 Windows。

## 8. 2026-11-08 检查与优化依据

基线：`c002b56c38d81bcd5044ddb22597a4e78ff2a88d`。以下是历史运行证据，不是本次修改后的远端性能结果：

| 证据 | 观察 | 对应调整 |
|---|---|---|
| [CI 42485909147](https://github.com/Open-Less/openless/actions/runs/42485909147) | 浏览器串行执行 923 秒；整轮约 16.9 分钟 | Chromium / WebKit 两个 runner、各 1 worker，保留原 `UI motion (Chromium + WebKit)` 汇总检查 |
| [CI 42402206738](https://github.com/Open-Less/openless/actions/runs/42402206738) | Windows 1518 秒，其中 MSRV 两步 274 + 151 秒 | MSRV 并行独立缓存；stable 与 MSRV 均为必需结果 |
| [CI 42445115251](https://github.com/Open-Less/openless/actions/runs/42445115251) | Windows MSRV 两步 225 + 122 秒；浏览器分拆在该分支已有运行证据 | 不扩大平台/架构集合，仅拆开现有串行工作；保留两浏览器的失败证据 |
| [桌面构建 42073710377](https://github.com/Open-Less/openless/actions/runs/42073710377) | Intel macOS 构建 2078 秒、Windows 1092 秒 | 不降低 release 优化级别；Windows NSIS 编译后，MSI 使用 `tauri bundle`，复用同一二进制 |
| [macOS 构建 42263206938](https://github.com/Open-Less/openless/actions/runs/42263206938) | Rust 日志 `No cache found.`，编译约 14m52s | 限制一次性 tag 缓存写入，保留 beta 的周期预热 |

超时证据：本次查询 `actions/runs?status=timed_out&per_page=10` 返回 0 条，不能据此认定历史故障都是超时。现有 Android APK 三个 job 未设上限，现补为计划 10 分钟、构建 90 分钟、发布 30 分钟；APK 构建步骤 70 分钟。浏览器单引擎步骤 20 分钟、job 40 分钟；下载、编译和测试等主要阶段各设步骤上限。这些是包含冷启动余量的防挂起边界，不是性能目标。前端脚本每文件最多 120 秒，打印文件名、耗时，超时返回 124；TypeScript 使用 `node --import tsx` 直接执行，减少启动进程，并避免超时只杀掉 tsx CLI 而留下测试子进程。Cargo `--timings` 与 Actions 步骤日志用于后续调整；进程被强杀时不保证生成完整 Cargo HTML。

npm 下载缓存使用锁文件精确键，缺失时按 OS/架构前缀恢复已有 tarball；仍执行 `npm ci` 校验锁文件和完整性。CI 优先使用本地下载缓存，并限制请求超时/重试。`node_modules` 不缓存。本仓库使用 npm 锁文件，没有新增 pnpm 流程。TypeScript 增量信息写入忽略的 `node_modules/.cache/tsc`，每次 Vite 仍重新打包。

本地复测：Linux、Node 24.22.1（远端 CI 仍使用 Node 22），三次均按 `npm run prebuild` → `tsc` → `vite build` 执行。关闭增量时 26.64 秒；删除增量信息后首次构建 27.75 秒；重复构建 13.96 秒，比非增量约少 48%。三次输出的全部 73 个文件 SHA-256 一致。收益针对重复构建，首次构建没有提速；该样本不代表 macOS/Windows 原生构建或 GitHub 整轮 CI 的加速比。

浏览器失败日志与 Cargo timings 保留 7 天；安装包和发布中转 Actions artifacts 保留 14 天。GitHub Release 已发布附件不受此期限影响。Android 相同 ref/ABI/profile/signing 的手动运行取消旧任务，tag 发布不被新运行中途取消。既有 PR 门控、完整平台集合与文档变更策略不变。

矩阵取舍：浏览器由一个串行 job 拆成两路，Windows MSRV 由 stable job 拆出，再增加两条轻量汇总检查以保留原必需检查名。单次全量 CI 的 runner 数会增加，目标是缩短关键路径；没有增加浏览器、平台或架构组合。实际墙钟收益仍受 GitHub runner 排队、冷缓存与新增缓存体积影响，需要合并后的同类运行对照，不能把各步骤节省相加作为整轮加速比。

本地验证：`npm test` 完成构建和全部 166 个测试文件；修改的工作流通过 actionlint；两种浏览器分别 `playwright test --list --project=…`，各发现 116 个测试。新增行为用例覆盖必需检查的失败/取消/跳过、挂起的 JS/TS 子进程、NSIS 失败中止和 MSI 二次打包。Windows 安装器实际打包、macOS 签名与 GitHub 多平台耗时仍需远端验证。
