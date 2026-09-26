# CI 触发范围与缓存配额

状态：canonical；更新：2026-09-26。本文说明 CI 为什么在什么情况下跑哪些 job、缓存在 10 GB 预算内怎么分配，以及发版前怎么把耗时压下来。

## 1. 触发与门控

| 事件 | 行为 |
|---|---|
| `pull_request` → `main` / `beta` | 按改动范围门控：改动够不到的平台直接跳过（见下） |
| `push` → `main` / `beta` | 全量验证（合并后的基线必须有完整证据），不跳过任何 job |
| tag `v*-tauri` | 发版：调用同一个可复用 Linux 打包工作流，并发布 deb/rpm |
| `workflow_dispatch` | 全量；`platform=macos` 只跑 macOS（仅 macOS 验证入口） |

同一 PR 连续推送会取消旧运行（`concurrency.cancel-in-progress`）。

### 改动范围判定

`openless-all/app/scripts/ci-changed-areas.sh` 对每个重平台 job 判定“改动物理上够不够得到它”，CI 的 `changes` job 调用它填 `changes.outputs.tauri` / `changes.outputs.msrv`：

| 区域 | 跳过条件（全部改动都命中才算跳过） | 覆盖的 job |
|---|---|---|
| `tauri` | 仅 `linux-egui/**`（但**不含**它的 `Cargo.toml`，那会动全工作区 lock）、`scripts/linux-fcitx5-plugin/**`、`docs/**`、`*.md`、`LICENSE` | macOS / Windows 桌面检查、Android 检查 |
| `msrv` | 全部改动里没有任何 `.rs`、`Cargo.toml`、`Cargo.lock`、`rust-toolchain*`、`.github/**` | macOS Rust 1.88 MSRV |

规则是**失败即运行**：没有基线提交（push / tag / 手动）、git 读取失败、diff 为空、区域名未知，一律返回 `true`。跳过只允许是“改动可达性可证明”的结果，不允许是“读不到差异”的兜底。判定逻辑有契约测试：`scripts/ci-changed-areas.test.mjs`（`npm test` 会跑）。

`linux-egui-package`（Linux Core/egui/deb+rpm）**不参与门控**：它是 Linux 产品和打包链的唯一验证，任何改动都跑。

## 2. MSRV 为什么是独立 job

- `openless-core`、`src-tauri`、`src-tauri/backend-tests` 都声明 `rust-version = "1.88"`，README 对外承诺该版本，因此必须有 CI 用真的 1.88 编译：MSRV 只会被“依赖升级抬高要求”或“自己用了更新的语言特性/API”打破，两者都只有在 1.88 下编译才会暴露。
- 工具链不同 ⇒ cargo fingerprint 全失效、产物与缓存不能共用。历史上 stable 与 MSRV 挤在同一个 job 里共用缓存，串行 29 分 59 秒（见 [`macos-build-performance.md`](macos-build-performance.md)）；拆成并行独立 job、缓存分开后是 stable 8m36s / MSRV 3m39s。
- 平台专属依赖图不同，所以 macOS 与 Windows 各自验证：macOS 是 `keyring/apple-native`、MLX/qwen-asr；Windows 是 `keyring/windows-native`、TSF IME。
- Linux 宿主 `linux-egui` 声明 `rust-version = "1.95"`（egui 0.36 的要求），因此 Linux 侧不存在 1.88 检查，README 已按此修正。

## 3. 缓存配额分配（10 GB / 仓库）

GitHub 每仓库保留 10 GB 缓存，超出按 LRU 静默淘汰；淘汰后 job 只会打印 `No cache found.`，表现为“无理由全量冷编译”。实测过的对照：`cargo test -p openless-core` 命中 148 s，冷启动 579 s。

实测体积与收益（一次命中节省 / 该 job 缓存体积）：

| job | 缓存 | 命中收益 | 性价比 |
|---|---|---|---|
| macOS Rust 1.88 MSRV | 0.72 GB | −13.1 min | 1102 s/GB |
| macOS stable | 0.88 GB | −11.4 min | 775 s/GB |
| Linux egui | 1.34 GB | −10.8 min | 483 s/GB |
| Android | 0.75 GB | −2.4 min | 188 s/GB |
| Windows | 1.62 GB | −4.6 min | 169 s/GB |

分配原则：**按平台配额 + 只有基线分支/tag/迭代中的 job 允许写。**

| 归属 | 目标 | 谁写 | 谁读 |
|---|---|---|---|
| Windows 依赖树 | 1.2–1.6 GB | `push`（beta/main）、tag | 所有 PR、发版 |
| macOS（stable 0.88 + MSRV 0.72 + MLX 0.30） | ~1.9 GB | 同上 | 同上 |
| Linux（egui + core） | ~1.7 GB | `push`、tag，**以及 PR 里的 `build-linux-egui`** | 同上 |
| Android | 0.75 GB | `push`、tag | PR |
| 发版（tag 作用域） | ~1.0 GB | tag / 发版前预热 | — |
| 余量 | ≥2 GB | — | — |

对应的机制：

- 重量级 job 用 `save-if: ${{ github.event_name != 'pull_request' }}`：PR 只从基线分支恢复，不再各自复制一份 1.6 GB 依赖树（此前两个 PR 就占了 10.87 GB，把基线缓存挤掉）。
- `build-linux-egui` 保留 PR 内自写并开 `cache-on-failure: true`：失败的运行也会把依赖缓存存下来，下一次迭代仍然热；同时关掉 dev/test debug info，缓存更小、链接更快。
- key 结构为 `v0-rust-<job>-<os>-<环境哈希>-<lockfile哈希>`：环境哈希含工具链与 `CARGO_PROFILE_*`，所以 stable / MSRV / release 天然分开，不要用跨平台 `shared-key` 混合。
- `Cargo.lock` 变化时精确键失效，但 `restore-keys` 前缀回退会拿到同前缀的旧缓存，所以“锁变了”不等于“全冷”——前提是那份没被淘汰，这正是要守预算的原因。

## 4. 维护

- 报告与清理：`bash openless-all/app/scripts/ci-cache-usage.sh [--prune] [--limit-gb 8] [--repo OWNER/NAME]`。`--prune` 只删已关闭/已合并 PR 的 `refs/pull/*` 作用域（先查 PR 状态，状态读不到就保留），永不触碰分支与 tag 作用域；用量超过预算时退出码为 1。
- 自动维护：`.github/workflows/cache-maintenance.yml` 每周一 03:17 UTC 运行并支持手动触发，预算 8 GB。
- 合并/关闭 PR 后可手动立即回收：`gh api -X DELETE repos/Open-Less/openless/actions/caches?ref=refs/pull/<N>/merge`。

## 5. 发版耗时

`Release Tauri (cross-platform)` 的基线是 ~32.7 分钟，关键路径是 macOS Intel 构建。原因是 tag 运行只能读默认分支（`beta`）的缓存，而基线分支跑的是 debug/test profile，不产生 `macos-release-v1` 前缀的 release 缓存，因此每个新 tag 都要从零编译 release 依赖。

**发版前先预热**：把一个 `Release Tauri (cross-platform)` 运行 dispatch 到普通分支（上游历史上就在 `perf/macos-ci-build` 这样做过，单次 11–19 分钟），预热完成后再打 tag，可把发版压到 ~10 分钟量级。

Linux 发版不受影响：`release-linux-egui.yml` 复用 CI 的同一个 `build-linux-egui` job（同名同 key），tag 运行能恢复基线分支的缓存。

## 6. 已知取舍

- 改动范围门控只跳过“够不到”的平台；`push` 到 `main`/`beta` 始终全量，所以合并后仍有完整证据。
- `build-linux-egui` 在 `push` 时仍会构建 release 二进制（保持基线缓存里有 release 产物、并持续验证打包链），但不再上传 Action artifact；artifact 只在 PR 与 tag 运行产生。
- 所有 job 都设了 `timeout-minutes`，避免卡死的运行长时间占用并发与时间。
