# #1338 前端正常、后端相关操作卡住：观察轮次 A0

## 范围与证据资格

本轮基线为上游 `Open-Less/openless beta` 的 `c002b56c38d81bcd5044ddb22597a4e78ff2a88d`，不是用户现场安装提交。用户描述：快捷键不响应/不稳定，随后发现大模型配置无法加载、权限检测失败、托盘退出无效；前端界面仍正常。发现顺序不等于稳定触发步骤。现场 Windows 精确版本、OpenLess 安装版本、故障时刻和前后端进程存活情况未知。

只读输入是 `openless-2026-11-06T14-37-57.log`，累计多日、11995 行。原文件不更改、不上传全文；不把文件名当作故障时刻或安装版本。本轮不能以静态代码推断确认根因，因此只允许观察候选，不先改锁、调度、权限、超时、重试、退出或业务行为。

开发在 `D:/Dev/worktrees/openless/issue-1338-backend-hang`，分支 `fix/issue-1338-backend-hang`；不动原克隆。所有测试、安装依赖及构建仅在 Actions；提交前独立 `review-diff-bugs` 审查到 `no bug found`，仅推 fork，不提 PR、不发 release。

唯一会话日志预留为 `%LOCALAPPDATA%/OpenLess/diagnostics/openless-issue-1338-20261106-session.ndjson`，只追加、不覆盖、不轮转；调查过程不创建现场日志。A0 不是行为修复，不满足原症状验收。此后有效的单变量修复必须保留同一探针与会话文件，使用新的 attempt/change 身份。

## 正常行为模型与已有证据语义检查

| Contract | 期望及来源 | 可检查信号/边界 | 已有证据分类 |
|---|---|---|---|
| N1 | 用户触发快捷键应得到相应功能的可见结果（用户需求）；合法禁用、取消和冲突单独区分 | 物理按键→监听→业务→可见结果；具体性能预算未定义，不编造秒数 | 用户反馈 VIOLATION；日志对本次请求关联 INSUFFICIENT_EVIDENCE |
| N2 | 打开模型配置应得到配置或明确失败，而非无结果（用户需求） | 请求、后端服务、响应、前端应用；不记录配置值 | 用户反馈 VIOLATION；日志无关联边界，INSUFFICIENT_EVIDENCE |
| N3 | 权限检测应返回真实状态或可辨认失败，不能把卡住当权限被拒绝（用户需求） | 后端检查进入/原生调用/返回；平台不适用是合法结果 | 用户反馈检测失败；具体错误类型和阶段 INSUFFICIENT_EVIDENCE |
| N4 | 用户主动托盘退出应能结束应用（用户需求） | 菜单回调、退出请求、清理、进程终止（外部观察） | 用户反馈 VIOLATION；日志无关联退出请求，INSUFFICIENT_EVIDENCE |
| N5 | 前端纯界面正常不等于后端健康；局部调用成功不等于后续全局健康 | 分别观察纯 UI、IPC、执行和可见结果，不用 heartbeat 替代业务进度 | 用户澄清前端正常；11983/11987 局部完成不能否定之后卡住 |

状态版本、重复副作用和计数守恒不是当前已知症状，本轮不虚构该类契约。缺少经验证的日志覆盖、flush、请求对应关系和观测截止时刻，不能用缺少事件证明代码未执行。

有限日志线索（UTC）：

- L11570：08:57:23Z，选区润色快捷键注册失败且重试计数 5340。只说明该时段的注册异常；不是本次跨功能卡住的已证实起因。
- L11983/L11987：14:29:42Z–43Z，有同一次语音流完成及插入 `PasteSent` 的后端局部记录；不证明用户界面内容正确，也不证明其后健康。
- L11989：14:37:22Z 出现启动记录；不证明此前如何终止。
- 其他时段 `sign_in_required` 不能视作本次系统权限检测失败。

即使删去所有 ERROR/WARN，用户报告的 N1/N2/N4 仍是行为异常；现有 INFO 不足以确认其因果边界。当前没有能稳定复现原故障的红色反馈环。

## 初始可证伪假设（静态线索不是结论）

以下是排名初始集，接缝需以实际实现和最终探针表校准。每个假设当前均为 `INCONCLUSIVE`，尚无 `ACTIONABLE` 行为修复资格。

| ID | Hypothesis | Falsifiable prediction | If true, what change would make the bug disappear? | Probe IDs |
|---|---|---|---|---|
| H1 | 同步 `get_settings` 的 preferences 读取或 warm-capture configure/stop 阶段阻塞了原生调用派发，使后续读取及托盘事件不能继续（N2/N4/N5） | 同一生成线程的同步配置命令在 preferences 或 warm configure 阶段进入后未返回，其他原生主线程任务未执行；需结合完整覆盖及现场线程栈排除捕获损失 | 只将证实阻塞的该同步操作移出主线程且保持结果/顺序语义，原跨功能停滞应消失 | P1/P2/P4/P6 |
| H2 | 权限原生麦克风检查在异步 worker 内同步阻塞，导致其他异步服务没有执行机会（N2/N3/N5） | 原生 mic 阶段进入后停住，异步命令已派发但执行/完成停滞，而原生事件循环仍可执行；必须结合线程栈/执行资源观察区分孤立慢调用 | 只把证实阻塞的 mic 检查迁至合适的 blocking 执行边界，其他请求应恢复且权限语义不变 | P1/P2/P5/P6 |
| H3 | 凭据共享锁持有路径停滞，配置读取等待该锁，并可能占住调用执行资源（N2/N5） | 生成锁操作显示已持有而其他操作等待；命令与该锁必须有真实父子/同线程调用关联；原生权限可独立返回时可否定全局卡住归因 | 只缩短已证实持锁路径或修正已证实锁环，保留凭据授权/缓存/写入原子性，配置读取与其他请求恢复 | P2/P3 |
| H4 | 后端任务在原生主线程派发边界等待，托盘事件也不能进入；不是前端渲染冻结（N1/N4/N5） | 真实主线程排队已接受但执行边界停滞，现场栈显示该队列被占用；原生事件回调/退出进入边界缺失只有覆盖完整时才可用 | 只移除已证实的主线程阻塞或同步重入等待，保持窗口资源所有权与业务取消语义 | P1/P4/P6 |
| H5 | 同一前端页面缓存的 startup readiness 未完成，普通命令在前端共同前置阶段等待，尚未真正发送（N2/N3/N5） | 原生 `get_startup_snapshot` 已开始而未完成，现场前端 ready Promise 仍 pending；需要前端现场检查，不把后端缺少命令入口等同于未发送；若同一页面 ready 已 fulfilled，则不能用后来挂起解释它再次 pending | 只修正证据定位的 startup 阶段阻塞，保持恢复/凭据与生命周期安全边界，普通调用应开始发送并返回 | P1/P2（前端状态覆盖未知，须另取现场） |

源代码核对：`src/lib/ipc/shared.ts:48–63,77–95` 将普通调用放在缓存的 readiness 后；`src-tauri/src/commands/dictation.rs` 的 startup 命令调用 `crates/openless-core/src/api.rs:4967–4997` 的 `start()`，包含 lifecycle gate、偏好、凭据预热与恢复，而非纯存活检查。`commands/settings.rs:24–28,242–256` 的同步设置读取还会调用 warm configure；`recorder.rs:1353–1360,1388,1405` 涉及 lifecycle gate 与 stop。`core_adapters.rs:1642–1663` 的权限 future 内含同步原生调用；`permissions.rs:441–442` 的 Windows accessibility 是 NotApplicable，而 `:451–471` 的麦克风会探测设备/建流。权限前端 `Promise.all` 可因一项悬挂而整组不更新，因此不能仅凭权限 UI 推断所有 worker 饥饿。以上均为基线源码契约，不是运行时因果确认。

## 探针约束

最多六个真实边界：P1 原生 invoke 派发；P2 命令执行/服务等待；P3 凭据锁等待/持有；P4 到原生主线程队列的真实排队/执行；P5 权限原生检查；P6 托盘退出/应用生命周期。最终实现必须标明支持的命令、线程/父子标签和未覆盖部分。

仅后端观察不覆盖前端请求意图、前端发送失败、页面生命周期或响应应用结果。不得把后端没收到当作前端没发送，也不得把 dispatch 返回当作异步业务完成。日志导出不依赖挂住的 IPC：使用文件系统取得唯一会话文件；若当前 writer 健康/尾部 flush 不可观察，明确为覆盖未知。没有前端诊断 transport，不能虚报前端丢失计数为零。

探针必须默认关闭、仅显式分支诊断 Windows 包启用；所有事实只用闭合枚举、布尔值、计数、生成的运行内操作/线程标签和单调时长。不得含命令参数、模型/渠道/设备/窗口标识、路径、内容、凭据或原始错误。生产热路径不做序列化、磁盘写入、同步等待或新增延迟/重试；队列必须有界且损失可辨认，不宣称 lock-free。取消/unwind 仅算 scope-left，不能当成功。

## A0 实际实现与覆盖边界

- Cargo feature `issue-1338-diagnostics` 默认关闭，Windows 显式启用后才初始化真实 writer；不是旧 `issue-1266-diagnostics` 会话。
- P1 仅 allowlist：`get_startup_snapshot`、`get_settings`、`list_channels`、`read_credential`、两项权限检测。handler 的 bool 只表示派发/处理返回，不是 async 完成；其他 IPC 无覆盖。
- P2 观察以上命令的服务执行，设置读取区分 PrefsRead/WarmConfigure；凭据 blocking worker 使用显式生成的 operation。startup 仅命令整体，不覆盖 Core 内部每个恢复子阶段；`get_settings_snapshot` 当前不在覆盖内。
- P3 包装实际凭据总锁和 cache 锁，释放记录在真实 unlock 后；不改变锁顺序/生命周期或凭据内容。其他锁没有被凭据锁探针覆盖。
- P4 host/capsule 主线程包装区分 requested、dispatch_returned、entered、returned/scope_left；requested 可能早于 handle 获取，不能仅凭 requested 认定成功排队。
- P5 观察导出的原生权限检查进入/返回和闭合 PermissionStatus；不观察所有平台内部 helper，也不细分底层设备/建流/play/drop 的每一步。要进一步区分底层卡点须结合现场栈，而非凭单一进入记录猜测。
- P6 观察 tray quit、request_quit 和既有 Ready/Close/Destroyed/ExitRequested/Exit 回调。保留上游已有 3 秒强退兜底；没有增加产品延迟。Exit callback 本身不证明进程终止。
- async 观察只在每次 poll 的同步栈安装/恢复 TLS，不跨 await 维持线程上下文。缺少真实父关联显示 unknown，不以邻近记录补关联。
- writer 容量 512、`try_send`，各记录携带健康快照；`tail_capture=unknown` 始终保留，flush 为 OS 写入而非 fsync。现场可能有最终未落盘记录或初始化/磁盘失败；过去的零丢失不是完整覆盖证明。
- 可选 `issue1338_health` 仅诊断 Windows 包注册，独立读取原子状态；仍依赖 IPC，卡住时不保证可用。直接从文件系统取得会话文件，不以普通应用日志导出代替。

### 分支构建与交付约束

`Release Tauri (cross-platform)` 手动指定当前分支、`platform=all`、`windows_backend_diagnostics=true`、`windows_hang_diagnostics=false`，`release_tag` 留空。两种诊断会话互斥。Windows artifact 名为 `openless-windows-x64-issue-1338-diagnostics`，包含 source provenance 与匹配 EXE/PDB 验证清单；macOS 是普通构建。CI 显式编译 feature-on 宿主并运行导入真实模块的 off/on 隔离测试；这些测试不打开现场会话文件。测试通过只证明诊断层契约，不证明原设备问题修复。

同时为准确 head 运行分支 CI（含 Linux 包门禁）及 Android 全 ABI 分支构建，不创建 PR/tag/release。验证 run head、实际 job、非空未过期 artifact 后才交付；artifact archive digest 不等于内部安装文件的 checksum。

## 验收与下一步

A0 先接受诊断本身的安全性、边界覆盖、默认关闭和 Actions 产物来源；不宣告修复。有效行为修复须取得对应现场/探针证据，选一个可逆因果变量，验证同一 N1–N5 用户可见结果及退出结果，再经用户确认后清理探针。两次不能重现并不自动证明有效。
