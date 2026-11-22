# CI 触发、平台检查与缓存

[CI 工作流](../.github/workflows/ci.yml)定义运行条件、runner 与命令。[发布规范](../RELEASING.zh.md)定义打包、签名与发布门槛。

## 触发与门控

| 事件 | 行为 |
| --- | --- |
| PR 指向 `main` 或 `beta` | `Changed areas` 按改动范围选择平台检查。`ci:desktop-only` 在下次 PR CI 时跳过移动端和浏览器矩阵。标签变更本身不派发 CI。 |
| 推送 `main` 或 `beta` | 无 PR 比较基线，运行完整矩阵。 |
| 推送其他分支 | 通用 CI 由 PR 或手动运行触发。 |
| 手动运行 `CI` | 默认 `platform=all`。`platform=macos` 只运行 macOS 检查，可指定 `macos_runner=macos-15-intel` 或 `macos-latest`。 |
| `v*-tauri` 标签 | 独立发布流程。Linux 包单独验收并由管理员处理。 |

并发控制会取消同一 PR 的旧 CI，或同一 ref 与参数组合的旧手动 CI。默认分支基线、标签发布和草稿修复有各自的并发策略。无发布权限的验证或 Dev 打包使用分支工作流产物。

[`ci-changed-areas.sh`](../openless-all/app/scripts/ci-changed-areas.sh)调用 [`ci-impact.mjs`](../openless-all/app/scripts/ci-impact.mjs)分析改动。Rust 区域按 Cargo manifest、路径依赖和目标 `cfg` 闭包判断。前端、工具和原生工程由各自路径规则覆盖。Git 比较、Cargo 图或路径分类不确定时运行检查，只有明确的 `false` 可以跳过。

| 区域 | 主要检查 |
| --- | --- |
| `tauri` | macOS、Windows stable 构建与平台契约。macOS 包含原生启动／窗口验证所需的输入，Windows stable 包含 MSI 与 TSF 路径。 |
| `msrv` | macOS、Windows 分别用工作流固定的 Rust 1.99 验证。`Windows checks` 按是否应运行聚合 stable 与 MSRV。 |
| `frontend` | Chromium、WebKit 分开运行 `test:ui-motion`，聚合作业反映任一必需浏览器的失败。失败保留 trace 和截图。 |
| `android` | Android 目标编译、Kotlin／清单契约以及工作流配置的设备或模拟器检查。 |
| `harmony` | ArkTS 契约、Rust 宿主与 HAP。常规 CI 的 HAP 不签名。 |
| `capsule` | macOS 与 Windows 的原生胶囊独立检查，覆盖共享 crate 的 `ui,host` feature。 |
| `ios-device` | `test` 与 `device-build` 由 Xcode runner 分别验证，保留共享契约、模拟器与 iOS 真机目标编译的各自证据。 |

Linux 专属 CI 与自动发行已暂停。Linux 验收使用本地构建、Fcitx5 插件、打包脚本与[设备矩阵](linux-egui-handoff/07-acceptance.md)。Ubuntu 上的浏览器、Android 和 Harmony 检查独立运行。浏览器夹具和编译结果与系统权限、已安装应用、真机交互分别取证。

## 缓存

- `beta` 的合格 CI、手动构建和周期构建负责填充共享缓存，按各工作流的条件执行。PR 和常规 tag 发布以恢复为主。旧标签的修复可能使用源版本自带的缓存入口。
- Playwright 与移动端 Rime 使用分开的 restore/save 步骤，Rust 通过 `save-if` 门控。npm 复合 action 只在允许写入的基线运行启用自动保存，其他运行使用 restore。
- Rust 分平台和工具链缓存。macOS stable、MSRV 和 release 的编译配置不同，不能根据同一个 `shared-key` 推断命中。锁文件变化可以回退到旧前缀缓存，但不会重用不匹配的 Cargo 产物。Windows stable 与 MSRV 并行，前端验证复用 stable runner。
- Playwright 以浏览器引擎和版本拆键。Harmony SDK 从固定校验和的归档准备，Rust host 依赖单独缓存，避免 SDK 占用缓存预算。
- macOS 打包参数从 [`macos-build-env.sh`](../openless-all/app/scripts/macos-build-env.sh)加载，包含 codegen、strip 和 proc-macro host wrapper。正式包走 [`build-mac.sh`](../openless-all/app/scripts/build-mac.sh)，其中 Apple Silicon 的 Finder 布局在 DMG 压缩前写入并验收。

## 维护与验证

[缓存维护工作流](../.github/workflows/cache-maintenance.yml)每天 03:17 UTC、`main`／`beta` PR 关闭后以及手动运行时执行。它按 8 GB 维护预算回收关闭 PR 的缓存、每个分支缓存族旧于两代的条目，以及继续超额时最久未使用的分支缓存。开放或状态不可读的 PR 与 tag 不参与清理。闭合事件读取默认分支脚本。删除需要 `actions: write` 与 `pull-requests: read`，由维护作业持有。

从仓库根目录预览清理计划：

```bash
bash openless-all/app/scripts/ci-cache-usage.sh --prune --keep-family 2 --prune-budget --limit-gb 8 --repo Open-Less/openless --dry-run
```

区域与缓存策略由 `ci-changed-areas.test.mjs`、`ci-cache-sharing.test.mjs`、`ci-cache-usage.test.mjs` 及 `ci-consolidation.test.mjs` 检查。MicYou 更新由独立的[上游导入工作流](../.github/workflows/sync-micyou.yml)提出草稿 PR。源码 pin、补丁与校验流程见 [MicYou 上游说明](../openless-all/app/crates/micyou-dsp/UPSTREAM.md)。
