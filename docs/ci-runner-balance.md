# CI runner 数量与耗时取舍

## 当前决定

- Linux egui 专属 CI 与自动发行暂停：删除两个 workflow 和桌面 tag 自动派发；源码、本地打包和包校验脚本保留。
- Windows 平台敏感契约合入现有 stable job，复用已有 checkout、npm 安装、前端构建和 PowerShell 解析，不再单独启动 Windows 前端 VM。
- iOS 测试与真机构建复用统一变更检测，但独立运行：两者不共享产物，不必串行。保留 `test` / `device-build` 检查名；两项都应作为合并门禁，不能仅要求其中一项。
- 不增加大规格收费 runner、不增加浏览器 worker 或 Android ABI。浏览器双引擎、stable/MSRV、胶囊继续保留独立缓存/任务，避免强行合并造成关键路径重编译。

## 实际使用的机器

以下来自 GitHub job 日志，不是仅根据 `latest` 标签推测；镜像会随 GitHub 更新。

| 标签 | 日志中的镜像/系统 | 主要用途 |
|---|---|---|
| `ubuntu-latest` | Ubuntu 26.04（维护/同步任务日志） | 改动检测、Android、鸿蒙、维护与发行协调 |
| `ubuntu-24.04` | Ubuntu 24.04.5 | Chromium/WebKit 浏览器测试；Linux egui 原生任务现已移除 |
| `windows-latest` | Windows Server 2025 / `windows-2025-vs2026` | Windows stable、MSRV、胶囊与发行 |
| `macos-latest` | macOS 26.7.2 / `macos-26-arm64` | macOS stable、MSRV、胶囊、ARM 发行 |
| `macos-15-intel` | macOS 15.7.9 / `macos-15`，x86_64 目标 | Intel 桌面发行、手动 CI 回退 |
| `xcode-27` | macOS 27.0.1 / `xcode-27-arm64` | iOS 测试与真机构建 |

抽查的 runner 日志标明 Hosted Compute Agent：Linux/Windows 在 Azure，ARM Mac/iOS 在 MacCloud。日志未记录完整 CPU/RAM 规格，本次不把标签当作硬件配置保证；也没有取得仓库自托管 runner 清单权限。

## 修改前的实测样本

时间是 job 的 started/completed 差值，不含 job 启动前排队；不能把它们相加当成整轮墙钟耗时。

| 样本 | job | 时间 |
|---|---|---|
| [CI 42646377148](https://github.com/Open-Less/openless/actions/runs/42646377148) | macOS stable | 920s，其中 Rust 后端 393s、Codex 沙箱契约 246s |
| 同上 | Windows stable / 前端 / MSRV | 511s / 95s / 197s；前端契约本身 42s |
| 同上 | WebKit / Chromium | 584s / 468s |
| 同上 | Android / 鸿蒙 | 614s / 508s |
| 同上（已停用） | Linux 测试 / 打包 | 602s / 420s，两台 VM 共约 17 VM-min |
| [iOS 42588075524](https://github.com/Open-Less/openless/actions/runs/42588075524) | 测试 / 真机构建 | 475s / 879s，原先串行 |
| [桌面发行 42609582726](https://github.com/Open-Less/openless/actions/runs/42609582726) | ARM Mac / Intel Mac / Windows | 386s / 1740s / 1747s |
| [Android 42609582763](https://github.com/Open-Less/openless/actions/runs/42609582763) | armv7 / aarch64 / x86_64 / i686 | 800s / 827s / 1075s / 1099s |
| [鸿蒙发行 42613396802](https://github.com/Open-Less/openless/actions/runs/42613396802) | HAP | 572s，其中工具链准备 130s、构建 412s |

## 预期，而非已验证收益

- Windows 合并会给 stable 增加约 42s 契约工作和少量前置检查，但省掉一台约 95s 的 VM及重复安装/构建。该样本下 Windows 仍低于 macOS 920s 关键路径；缓存变冷时需重新测量。
- iOS 两台 VM 总数不变，最大并发增加一台；不再等待 475s 测试完成后才启动 879s 构建，预计这部分墙钟从约 23 分钟变成约 15 分钟。若测试快速失败，设备构建可能已经消耗一部分计算，这是并行化代价。
- 删除 Linux 专属 CI 每次全量少两台 Ubuntu VM，按样本省约 17 VM-min；因为其任务不是该轮关键路径，不能宣称整轮直接少 10 分钟。停止 Linux 发行也不再启动对应构建/发布 VM，但样本中的 tag 尚未派发 Linux，不能虚构这轮已经节省了多少。
- 旧全量组合：主 CI 的 16 台执行 VM，加独立 iOS 的 3 台，共 19 台。新全量 CI 配置展开为 15 台：iOS 统一改动检测省 1、Windows 前端合并省 1、Linux 测试/打包移除省 2。该数字是每轮启动总数，不是峰值并发；PR 仍按改动范围跳过任务。
- 桌面发行关键路径仍是 Intel Mac/Windows 的约 29 分钟编译。不能仅靠合并短协调 job、删除 Linux CI 或提高 timeout 改善，后续需要相同 profile/工具链的缓存命中证据再优化。

## 参考 PR #1412 的进一步优化

[PR #1412](https://github.com/Open-Less/openless/pull/1412) 的首轮把执行窗口从 21m14s 降至 14m28s，但未加权 runner 用量从 82m40s 增至 94m20s。第二轮产物复用减少了重复编译条目，整轮却因 Windows 波动变为 17m15s。因此延续其 profile/target 复用，不继续无条件扩张 VM。

已存在的浏览器分路、独立 MSRV、feature-on 诊断 `--profile test`、Windows backend 共用 target，以及 NSIS 二进制复用均保留。本轮新增：

- macOS 沙箱契约和 Windows CPAL/COM 测试显式使用 `src-tauri/target`，不再分别在 workspace/vendor 下生成依赖树。
- 诊断 off/on、所有权 helper、真实 Windows runtime、录音模块等临时源码 harness 通过步骤级 `CARGO_TARGET_DIR` 复用已恢复的主机 target。源码仍是隔离的临时工程，不改测试路径、features、锁文件或断言；仅复用 Cargo 能证明版本/features/profile 相同的产物，不保证每一个包都命中。
- 所有权脚本尊重传入 target，并从同一位置寻找执行文件。未传变量的本地运行仍使用原先隔离目录。
- Windows 手动运行新增 `windows_build_jobs=2/4`，默认 push/PR 和 macOS 仍为 2。concurrency group 包含该参数，使两组手动对照不会相互取消；没有额外新增 job 或 runner。该选项是同一 VM 内的 Cargo/CMake 并行度，不是 VM 台数。当前手动入口仍运行所选平台的既有 CI 范围，不应把两轮全量对照视为免费实验。
- stable job 记录 CPU 型号/核心数、内存、Rust/Cargo 版本和编译上限，便于排除机器规格、缓存冷暖差异。仅在同一提交、相同 runner/缓存条件下对比后，再决定是否提高默认并行度；默认不冒 OOM 风险。

### 结合 PR #1417 的 iOS 补充

参考 [PR #1417](https://github.com/Open-Less/openless/pull/1417)，将 host/device 独立 Rust 缓存、长步骤超时及 7 天 Cargo timings 合入现有 `ci.yml`，不恢复独立 iOS workflow。Host target 映射 `CoreHost/target`，device target 映射 `.build/core-ios16`；缓存族区分 host/device，包含 Xcode identity、Rust/lockfile 身份，device 再区分 iOS 16。仅 beta push/手动运行写入，PR 只读；生成的 Core 源码、模型和 CMake 二进制不缓存。新缓存体积及预热收益需线上测量，并受既有 8 GB 维护预算约束。

下载保留断点续传与 SHA-256 校验，同时设置连接、总时间和低速限制；Boost 先下载至临时文件，校验后才替换正式归档。验证覆盖首次成功、超时续传、持续超时退出、校验失败拒绝发布、已验证文件复用。PR #1417 的远端验证是该 PR 自身的证据，不替代本次迁入统一 CI 后的远端验收。

### 本地验证与待验证

- 修复了既有 `ci-performance.test.mjs` 的机器配置泄漏：清除 `CARGO_TARGET_DIR` 并不能覆盖 `~/.cargo/config.toml` 的 `build.target-dir`，先前第一轮构建被重定向到全局目录，第二轮却显式使用 fixture 目录，导致 `fresh` 断言失败。现在所有离线 fixture 命令都显式使用同一临时 target；复用、非测试生产构建及故意损坏的 feature 编译失败用例全部通过。
- `npm test` 完整通过：生产前端构建及 170 个 JS/TS 测试文件（包含新增的复用/合并、iOS 下载、签名汇总与双源下载回退契约）。
- 4 个 Python harness 路由/模式用例通过；真实 Rust 所有权 helper 的 7 项单测及 2 项编译失败边界在指定 target 下通过。
- Actionlint、相关格式与 `git diff --check` 通过。完整 Windows runtime/录音/COM 原生执行和 macOS 沙箱需要对应平台的 Actions 验证。

上述结果为本地验证，尚未完成修改后的完整远端流水线验收；本轮不新增 VM，不承诺未经测量的整轮提速百分比。
