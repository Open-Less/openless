# qwen-asr 子模块升级清单

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。

## 1. 子模块（`.gitmodules`，仓库根）

| 子模块 | 路径 | 来源 |
| --- | --- | --- |
| qwen-asr | `openless-all/app/src-tauri/vendor/qwen-asr` | <https://github.com/Open-Less/qwen-asr.git>（组织 fork；`antirez/qwen-asr` 只作上游同步来源，不直接进构建） |
| qwen3-asr-rs | `openless-all/app/src-tauri/vendor/qwen3-asr-rs` | <https://github.com/Open-Less/qwen3_asr_rs.git> |

qwen-asr 是 macOS 本地 Qwen3-ASR 的 **C 引擎**，由 src-tauri 构建链接（vendored）；qwen3-asr-rs 为 Rust 运行时来源。Linux workspace（openless-core + linux-egui）不包含 src-tauri，不依赖这两个子模块。

## 2. 升级步骤

1. `git submodule update --init --recursive`（先在干净工作区）。
2. 在子模块目录 `git fetch && git checkout <目标提交>`（只用 Open-Less fork 的提交；记录提交号）。
3. 主仓 `git add` 子模块指针并提交（提交信息注明引擎版本/目的）。
4. macOS 构建验证：`src-tauri` 按平台构建通过；如涉及 Metal/工具链，先跑 `npm run check:macos-metal-toolchain`；相关契约：`scripts/macos-compiler-runtime-contract.test.mjs`、`stage-macos-mlx-metallib.mjs`（MLX 路径）。
5. CI 注意：`scripts/ci-disable-macos-qwen3.mjs` 控制 macOS CI 中 qwen3 的启用范围，升级后核对其仍符合当前 CI 策略。
6. 行为验证：本地 ASR 下载/激活/取消与听写回归（见[桌面验收](2.0-desktop-acceptance.md)本地模型域）。

## 3. 纪律

- 不修改 vendored 引擎源码本身的问题；修复走 fork 上游，再 bump 指针。
- 子模块指针升级必须与构建验证同批提交，不留"指针先行"状态。
