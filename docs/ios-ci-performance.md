# iOS CI 耗时与缓存

本次补充 `.github/workflows/check-ios.yml`，与桌面/Android 优化草稿 [#1412](https://github.com/Open-Less/openless/pull/1412) 独立，二者可以分别合并。

## 历史证据

以下为 GitHub Actions jobs/steps API 的完成时间减开始时间，不包含首次 runner 排队。三次运行使用相同的串行结构，没有控制机器负载，不能直接作为加速比的对照。

| 运行 | host 测试 | 其中 Rust 测试 / Clippy | device 构建 | 首个 job 开始到最后完成 |
|---|---:|---:|---:|---:|
| [42447525487](https://github.com/Open-Less/openless/actions/runs/42447525487) | 555 秒 | 334 / 76 秒 | 637 秒 | 1195 秒 |
| [42502799269](https://github.com/Open-Less/openless/actions/runs/42502799269) | 580 秒 | 353 / 90 秒 | 931 秒 | 1516 秒 |
| [42502805940](https://github.com/Open-Less/openless/actions/runs/42502805940) | 509 秒 | 307 / 81 秒 | 1080 秒 | 1592 秒 |

后两轮是同一 head `121597d8` 的 PR 与手动运行。它们并非运行内的必需依赖；日常应优先复用 PR 结果。手动运行仍与 PR 隔离：取消 PR 检查后，手动运行的成功不会补回 PR 所需的检查结果。

## 改动

- `test` 和 `device-build` 使用各自的 checkout，没有交换构件。设备构建只等待区域判断，测试和构建并行；保留原来的两个 job 名称和独立失败状态。区域检测失败仍阻止设备构建，共享 Rust 变更仍需要 device target 编译。
- 同类型、同 ref 的新 PR/手动检查取消旧任务；默认分支 push 独立运行，维持缓存写入。增加与 PR 相同路径范围的 `main`/`beta` push 触发，避免只读缓存却没有基线写入者。
- host debug/test 与 device release 的 Rust 缓存分开；显式映射 `CoreHost/target` 和 `.build/core-ios16`，不缓存生成的 Core 源码。缓存键含 Rust 环境、lockfile 和 Xcode build identity，device 再区分 iOS 16 部署下限。只允许 beta push/手动运行写缓存，PR 只恢复。
- 为 Swift、Rust、Rime、模型下载、Xcode 编译设置步骤超时，保留原来的 30/60 分钟 job 上限。curl 同时限制连接时间、单次总时间和最低传输速度；模型下载保持断点续传和 SHA-256 校验。Boost 下载先写临时文件，校验后再替换正式归档。
- Cargo 测试和 device 编译生成 timings，成功/失败时上传，保留 7 天。未新增矩阵、签名、发布、模型归档或 CMake 二进制缓存。

## 验证与收益边界

下载行为测试覆盖首次成功、超时后续传、持续超时、校验失败及已验证文件复用。每个长步骤的超时与原有校验保持在同一失败链中。

删除串行依赖可以消除历史样本中 509–580 秒的测试等待，但收益取决于 macOS runner 是否能同时启动。并行本身不会减少 runner 总用时；两个新 Rust 缓存要在 beta 首次运行后才可供后续 PR 恢复，首轮 PR 不承诺提速。记录后续冷/热缓存运行的实际耗时和缓存体积，再按仓库既有缓存预算决定是否保留；不把排队变化计入构建收益。
