# beta.5 用户反馈修复与审查结果

范围：`0454dc56..0947c05a`（PR #1197），以及本次工作树修复。

**当前交接状态（2026-10-15）：** 已构建并安装 `2.0.0-beta-a.beta.6+build.20261015-be` 到 `/Applications/OpenLess.app`，包含最后的实验下载状态 CSS 隔离修复。已安装主程序与最终构建产物 SHA-256 一致：`7f0918b3012d273fb1ec61b3a041089c85acf9fcb04f73e5ce933bbf6076bba4`。源码未提交、未推送。按用户最新要求，后续测试由用户负责；Agent 停止继续测试，不回滚、不撤回当前安装，不继续操作 App。其余审查 Agent 已关闭，本轮编排记录在备份后清空，自动任务列表为空。

## 结论

首录音停止后一直 Processing 的高可信根因不是转圈动画：**指定名称的麦克风 + CPAL 0.15.3 保留回调 sender + 音频处理线程只等队列 EOF**，使停止录音永久卡在 `join`，后续多模态请求尚未发出。该等待由 **`44972015` / PR #1194（麦克风降噪）** 引入，已经存在于此次整合基线之前。

已改为显式 FIFO `Shutdown`：停止接受原生采样、暂停/释放外层输入流，再排空已接收音频并结束处理线程，不再等待第三方回调析构。没有通过超时隐藏转圈、丢弃 join 或提前返回来掩盖阻塞。

本机日志 `~/Library/Logs/OpenLess/openless.log` 中，2026-10-15 20:26 与 20:38（+0800）都能看到输入流启动/释放，但没有后续 Omni POST；非秘密偏好为 DashScope 多模态、指定内建麦克风、降噪开启、低延迟关闭，与上述分支吻合。麦克风权限为 granted。旧 cb# 日志已随新 worker 停用，不能把缺少 cb# 当成没有采样的证据。

CPAL 自身的原生引用环没有被篡改或通过升级依赖绕过；本轮修复的是应用等待其析构造成的死锁。真实设备资源、拔插和完整输入仍需后述实机验收。

## ToDo 状态

- [x] P0：检查录音/停止/转写/多模态/输出调用链、本地日志与偏好，追溯引入提交；修复音频处理线程退出协议，并验证保留原生 sender 时已接收 PCM 不丢失、停止正常完成。
- [x] 独立审查全部整合差异：语音/快捷键、核心、多模态、前端、egui 与构建边界；原始问题和本轮修复状态分开记录。
- [x] 所有平台无显式偏好时默认界面大小 100%；首帧 bootstrap 同步应用。用户主动保存的 90%/100%/110% 不被覆盖。
- [x] 100% 返回录音输入页：复用缓存平台能力、滚动在 layout effect 归零、修正条件 return 后调用 hooks、避免无变化的 ResizeObserver setState。双浏览器反复往返采样确认控件位置稳定。
- [x] 共享主页面/设置内容保留短退出淡化，快速 A→B→A 取消旧任务；页面只淡化，不缩放/平移文字。设置对整个不透明 workspace 淡出，不再只淡出被实色外层遮挡的子节点。
- [x] 桌面设置仅保留“返回应用”；顶部专用拖动条高于全窗设置，并按界面缩放换算原生尺寸。主设置原先就是原生红绿灯；真正的仿制灯位于 Less Computer，已移除并接入 NSWindow 装饰和原有 dismiss 清理流程。
- [x] 实验模型采用 Settings/全局指示器共用的状态与取消路径；取消调用对应实验开关，而不是已退役的 Qwen 下载接口。首字节前为不定进度，少量字节不再伪装成固定 0%；错误可见，关闭设置不丢失任务状态。
- [x] 实验模型三个条目预留位置，状态/进度/操作放右侧，开关位置稳定。浏览器回归确认开始下载不推动后续条目；错误使用平滑展开。
- [x] 快捷键只有点击当前按键一个录制入口，删除重复展开菜单。录制提示使用右侧 Base UI Popover，不顶开下面内容；重置、停用和独立的 Mac 听写键动作保留。键盘捕获不依赖 Portal 焦点到达的时序。
- [x] 普通/可搜索下拉统一 Base UI Select/Combobox；开关、展开、提示与录制弹层复用 Base UI Switch/Collapsible/Tooltip/Popover。长列表跨越 120ms 动态内容刷新后保持用户滚动；右优先并保留碰撞避让、键盘操作和 zoom 补偿。
- [x] 云同步状态通过 Collapsible 平滑推动后续元素；实验完成提示、云同步设置/自动提示、市场登录使用共享退出保留。共享 Modal 处理顶层 Escape，保留已被子层消费的按键和 busy 保护。
- [x] 110% 下多模态表单紧凑布局、禁用状态提示、快速模式往返后的高亮跟随及窄屏可访问性回归。
- [ ] 本轮构建的原生 App：真实 Fn 首次/连续/取消录音，设置麦克风预览→听写交接，实际 DashScope/其他服务请求与目标应用上屏；没有用浏览器 mock 冒充通过。
- [ ] 原生标题栏拖动/三色灯、真实下载零字节取消/断网/重启、Windows Foundry SDK 与设备拔插验收。
- [ ] Linux 新视觉基线：本机仅能生成/检查 macOS 的八张图片；Linux 对应旧图尚未刷新，不能宣称 Linux UI CI 已绿。

## 根因与附带修复

| 问题 | 引入记录 | 修复与证据 |
| --- | --- | --- |
| 停止音频线程永久等待 EOF；还可堵塞麦克风预览操作锁，使下一段录音无电平 | `44972015` / #1194，基线前 | 显式 Shutdown；直接编译生产 recorder 的 22 项无硬件回归通过，含保留 sender、排空 8 帧并有界结束 |
| 普通页面删掉 exit；全窗设置不透明外层遮住淡出、覆盖拖动区；快捷键重复录制入口 | `8c32db9e` | 恢复保留/取消生命周期，整体淡化，专用拖动条及单一录制入口 |
| 长列表更新时重置高亮、抢回选中项；新实验轮询扩大重绘范围 | 原手写菜单为 `66915405`；放大因素 `16a73b9d` | Base UI 统一组件、稳定条目 identity、共享快照去重；双浏览器长列表动态更新验证 |
| Qwen/Stepfun 启动失败后持锁再 cancel，重入同一非重入锁 | `9ed34dab` / #775；`17fe67c5` / #835，均为历史缺陷 | 先释放状态锁再清理；两个错误分支回归通过。不是本机当前多模态故障的直接候选 |
| TextStream stdout EOF 后取走 Child 并 wait，取消/超时看不到仍运行的进程 | `16a73b9d` | 短锁 try_wait 保留取消句柄；生产 Process 模块的 EOF-before-exit 子进程回归通过 |
| 编辑非活动同 provider 渠道误释放活动 runtime；迟到状态刷新可能清理新实例 | `16a73b9d` | 按活动 owner 清理、只串行 owner 交接、清理完成后才做异步状态刷新；Core 合同 34 项通过 |
| Foundry 缺失/截断缓存无法冷删除，反而忘记 receipt | `16a73b9d` | 删除完整性与加载完整性分离，验证受管路径并保留失败凭据；6 项临时文件系统回归通过 |

## 独立审查

Orca Run：`run_102ebb62f9c8`。审查与实现文件所有权分离：主 Agent 负责语音与前端；一个后续 Pi 任务仅修改 `api.rs`、`local_asr_contract.rs`、`native_model_receipt.rs`，其余审查只读。

| Agent / 结果 | 范围 | 固定报告 |
| --- | --- | --- |
| Codex，完成 | 首录音链路、日志、偏好白名单与提交追溯 | [语音审查](reviews/2026-10-15/voice.md) |
| Pi，完成 | Core、多模态、本地运行时、egui/构建差异 | [核心审查](reviews/2026-10-15/core.md) |
| Pi，完成后续独占修复 | 渠道 owner 与 Foundry 缓存删除 | [修复与回归](reviews/2026-10-15/core-fixes.md) |
| Pi，完成；替代失败的 Cursor/Codex UI 审查 | 固定 HEAD 的双浏览器动态对照与前端/测试审查 | [UI 审查](reviews/2026-10-15/ui.md) |

Cursor 因额度耗尽退出；第一次替代 Codex 因 workspace routing discovery 连续失败未完成审查，没有把它们计为成功。最终三个审查范围均有独立报告。报告中的“当前 worktree”是各自交付时的快照；例如 UI 报告写作之后又将快捷键录制提示移入 Popover，并补了真实键盘输入回归，当前源码/本清单优先。

## 本轮验证

以下表格主要记录安装前代码修复阶段的检查，命令在 `openless-all/app/` 执行；没有清除用户配置或读出 Keychain 秘密。随后已完成 beta.6 的真实 Release 构建与安装；后续设备测试现交由用户负责。

| 检查 | 结果 |
| --- | --- |
| `npm run build` | TypeScript/Vite 通过；原有大 chunk 提示仍在，不把提高告警阈值当作优化 |
| `node scripts/frontend-test-runner.mjs` | 121 个前端/契约测试文件通过 |
| `CARGO_TARGET_DIR="$PWD/src-tauri/backend-tests/target" cargo test --offline --locked -p openless-core --lib` | 1204 通过，0 失败，1 个显式外部 Codex 用例忽略 |
| Core `local_asr_contract` | 34 通过，0 失败 |
| `CARGO_TARGET_DIR="$PWD/src-tauri/backend-tests/target" python3 scripts/test-recorder-module.py --offline` | 项目既有隔离脚本直接编译生产 recorder 与音频处理器：37 通过，0 失败，其中 recorder 22 项含本次退出回归 |
| `cargo test --offline --locked --manifest-path src-tauri/backend-tests/Cargo.toml` | 46 通过，0 失败；包括生产 Process、receipt 和已有 Host 合同 |
| Chromium/WebKit UI | 全量 96 项中首次 90 通过，6 项暴露真实布局/输入或旧断言问题；修复后 6 项及 4 个视觉用例无更新参数重跑全通过；另补共享 Modal/Guide Escape 4 项通过。共 100 个独立 UI 用例覆盖；没有把首轮失败记成一次全绿执行 |
| 八张 macOS 深/浅色截图 | 已更新并检查；不带 `--update-snapshots` 的独立像素比较通过，未提高容差；Linux 旧图未冒充已更新 |
| 原生 Rust 类型检查 | `DOCS_RS=1 CARGO_BUILD_JOBS=2 cargo check --offline --locked --manifest-path src-tauri/Cargo.toml` 通过；该模式跳过 Sherpa 本机库下载/链接，不是最终生产链接验证 |
| 格式/补丁 | 变更前端文件 Prettier、目标 Rust 文件 rustfmt、`git diff --check` 通过 |

第一次完整 native `cargo test --lib` 曾受 Sherpa 1.13.2 预编译库下载的 TLS EOF 阻塞；直接 curl 同一公共下载地址当时也超时。后续已通过断点续传取得完整官方依赖，真实 Release `.app` / DMG 构建、签名与包校验通过，并安装到 `/Applications/OpenLess.app`；该阻塞现已解除。没有把构建成功当作完整 native 单元测试已执行。因此本轮复用项目既有 `scripts/test-recorder-module.py` 直接编译生产 recorder/音频处理器，并将生产 Process 模块挂入独立后端测试 crate，完成实际队列与子进程回归。隔离脚本仅替换 Core 的接口桥，不打开设备；没有用空壳实现或 docs.rs 模式冒充 native 测试成功。生产依赖版本不变，后端 lockfile 仅增加测试所需的既有 libc 直接依赖，没有顺带升级/降级其他包。

主要本轮日志：`/tmp/openless-regression-core.log`、`openless-regression-recorder-final.log`、`openless-regression-backend-final.log`、`openless-regression-frontend-final.log`、`openless-regression-full-ui.log`、`openless-regression-final-validation.log`、`openless-regression-presence-final.log`、`openless-regression-delivery-ui.log`、`openless-regression-downloads-final.log`、`openless-regression-native-check.log`。临时目录可能被系统清理；本文件和报告保留结论及复现入口。

## 后续发版接手（2026-10-16）

用户随后确认本机修复版无明显问题，并授权全面 Review、PR/CI/合并和 beta6 → 2.0.0 正式发布；前述“不提交/停止测试”为上一阶段安排，已由本次授权更新。当前本机 App 不替换；同构建哈希、运行状态和最新真实录音完成日志已核验。发版前 Review、新发现的发布链路修复及本机门禁详见 [release-readiness.md](reviews/2026-10-16/release-readiness.md)。

## 实机验收入口

beta.6 已按项目构建流程生成并安装；[原 beta.5 安装包](desktop-beta5-integration.md) 不包含这些修复。按用户最新安排，以下剩余设备验收由用户接手，Agent 不再自动执行：

1. 指定内建麦克风，保持用户的 DashScope 多模态与降噪设置，冷启动后 Fn 开始→结束；确认日志出现新增 `audio processor stopped (accepted frames drained)`，随后有 Omni 请求及最终上屏。
2. 连续录音/取消，再在设置中打开、滚动、关闭麦克风列表后录音；不应残留预览锁、橙点或 Processing。
3. 100%/110% 来回切页、拖动标题栏、使用原生窗口按钮；实验模型从零字节下载取消、关闭开关、离开设置和重启后复查。

这些步骤保留为用户接手清单，不是全部通过声明。此前已观察到真实录音完成处理线程退出、多模态请求和 `Inserted` 终态；其余界面与完整设备行为由用户继续确认。
