# Issue #1266 Windows「更新后使用中无响应」源码调研报告

> 调研日期：2026-10-25<br>
> Issue：[#1266](https://github.com/Open-Less/openless/issues/1266)<br>
> 调研工作树：`D:/Dev/worktrees/openless/windows-3`<br>
> 现场构建提交：[`af14756d3d9d1474fa7db782c332b61deef58688`](https://github.com/Open-Less/openless/commit/af14756d3d9d1474fa7db782c332b61deef58688)<br>
> 当前源码 HEAD：[`38fbe1c082e782e566e13769e4675a183ccc812a`](https://github.com/Open-Less/openless/commit/38fbe1c082e782e566e13769e4675a183ccc812a)

## 1. 调研范围、结论先行与证据边界

本报告只做现场源码和提交差异的静态审计，没有本地编译、测试、打包、依赖构建，也没有改动产品源码、提交、推送或公开评论。源码链接优先固定到 Issue 所指向的现场提交 `af14756d...`；因此读者可以按同一提交复核行号。

Issue 描述的是「应用使用中无响应、重启后恢复」，不是已确认的进程崩溃。Issue 没有提供卡住时的线程栈、等待链、挂起转储、稳定复现步骤或可信的更新前后对照。原始约 2.7 MB、多日累计的日志文件也没有提供；Issue 中的日志摘录不能替代对原文件的独立分析。因此，本报告不重数摘录中出现的行数/次数，不把最后一条日志当作根因，也不把一次构建与更新因果直接绑定。

**结论：目前不能从源码证明 #1266 的现场根因。** 静态源码给出以下几类可证伪风险，但它们对应不同线程和不同故障边界：

1. `settings_host_gate` 被多个热键监督器、设置事务和恢复事务共享；部分路径持有它时会等待主线程回执、原生注册或 Core 事务。它足以造成设置/热键收敛延迟，属于需要现场等待链验证的静态风险；当前代码没有展示一个可直接闭合的 `settings_host_gate` ↔ Core `settings_write_gate` 锁循环。
2. Toggle 热键的专用 bridge 线程用 `block_on` 串行等待 Pressed/Released 的完整业务流程。ASR、网络握手、停止录音、插入等流程变慢时，后续热键事件可能排队；这能解释“热键无反应”或停止延迟，但 bridge 不是 Tauri UI 主线程，不能直接等同“整个 UI 卡死”。
3. 录音停止、ASR/LLM、TSF/IME、文本回退有多段异步/阻塞等待，完整 stop/finish 没有一个覆盖全链路的统一总 deadline。它能解释业务流程长时间不结束，不能单独证明主消息循环冻结。
4. WAV 归档和音频处理的实时回调使用有界、非阻塞投递；归档满时丢归档 PCM，处理队列满时让本次 capture 失败并停止。停止阶段另有 `send(Finish)`、处理器 join 和归档 writer join，可能延长业务停止，但不是回调内等待磁盘的设计。
5. Windows IME submit 在 blocking pool 中执行，源码为窗口发现、单次 `SendMessageTimeoutW` 和 post-dispatch query 设定了预算，并在结果不确定时拒绝重放文本。预算是单次操作边界，不是整个听写 stop 的总 deadline。

**现场状态的优先判定：** 在卡住尚未退出时取得同一 PID 的两份 full dump，用与现场 EXE 匹配的 PDB 分析全部线程，是目前最直接的分层取证方式；它不是唯一可用的证据形式。两次栈相同只能提示停滞，不能单独证明死锁或因果。根因还需可闭合等待关系、受控对照或同等因果证据支持。当前根因分类保持 `INSUFFICIENT_EVIDENCE`。

## 2. 现场提交与 HEAD 的差异

现场构建的提交是 release 元数据准备提交。对现场提交到 HEAD 的差异核对后，共享 Core 的变化包括 `api.rs` 的听写启动日志、`dictation_engine.rs` 的 ASR 耗时日志和流式日志降级，以及 E2EE 阶段标记日志降级；不是只有一条新增日志。所核对的 Tauri 热键、录音、插入、IME、归档、事件实现和 Windows release 工作流均无差异。日志开销变化不能自动视为与时序无关，更不能作为已修复证明。

HEAD 新增的日志位于 [`api.rs`（HEAD，约 7815 行）](https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L7808-L7825)：

```rust
log::info!(
    "[dictation] start session_id={} target={:?} asr={} insert={} translate={} silence_after_ms={:?}",
    session_id,
    context.output_target,
    context.asr.provider_type,
    context.insertion.enabled,
    context.polish.translation_active,
    context.recording.silence_after_ms,
);
```

这条日志不在现场提交 `af14756d...` 中。因此，现场日志缺少 `[dictation] start session...` **不能证明现场没有进入听写启动**；它只是后续 HEAD 的观测增强。该差异不能作为更新导致卡死或修复卡死的证据。

现场 Windows 构建产物入口已找到：

- [Release Actions run 39922069809](https://github.com/Open-Less/openless/actions/runs/39922069809)，head SHA 为 `af14756d...`。
- Windows job `120309424739`、符号验证和 Windows artifact 上传均成功；artifact 入口为 [artifact 12488899532](https://github.com/Open-Less/openless/actions/runs/39922069809/artifacts/12488899532)，名称 `openless-windows-x64`。查询时尚未过期，API 到期时间为 `2027-01-21T08:04:22Z`（产物也可能被提前删除）。
- 已知构建侧 EXE SHA-256：`2089F2DC030CFD7B41C5C4920B6200C9F811088768CDDF9E0655B82527C9D445`。
- 已知构建侧 PDB SHA-256：`50986081311C3577EC399C4B4C9CFE126D659F0646094B5D52636A898B75A6A9`。

本轮没有下载/解包 artifact，也没有现场 EXE 哈希，尚未核对 PE-PDB GUID+Age；因此只能说“有原构建符号产物入口”，不能说符号已经和现场 EXE 匹配。

## 3. 正常行为 N 合约

下面的 N 合约是后续现场证据的判定基准，不是对 #1266 已经满足的运行时结论。N1 的源码依据见 §6.1；N2 见 §5.2；N3 见 §5.3；N4 的可恢复终态要求来自 Issue，状态机见 §6.2；N5 见 §6.3；N6 见 §7.1–7.3；N7 是 Issue 已明确的证据限制及 §7.4 的观察通道约束。源码规则定义预期，不证明现场满足；现有材料也没有给出可套用到整个 stop 流程的总时限。

| 合约 | 正常行为 | 现场可观测的最低证据 |
|---|---|---|
| N1 热键顺序 | 物理边沿和取消语义不得错序；bridge 按 FIFO 接收，Toggle handler 串行处理，Hold 分支另有 spawn，不能把二者的执行方式混为一谈。 | bridge 线程栈、事件时间戳、Core session phase 的顺序。 |
| N2 设置事务 | Core 先验证并持有 settings write gate，Host 应用显式 runtime target；副作用失败时按 receipt 反向恢复，不能把旧/新 target 混用。 | 设置事务返回、target、Core revision、热键 supervisor 状态和等待链。 |
| N3 主线程注册 | 主线程注册需要 acknowledgement 的路径必须有明确等待边界；超时后调用者可重试，迟到闭包不能无条件覆盖更新后的 target。 | 主线程栈、注册闭包是否执行、ack 时间、目标版本/取消状态。 |
| N4 听写停止 | Released 进入 stop 后，应按 Starting/Recording/Transcribing/Inserting 的状态机完成、取消或报错；不能把业务 worker 忙误报成 UI 主线程死锁。 | recorder、ASR/LLM、insertion、IME 线程栈及 Core phase。 |
| N5 TSF/IME 结果 | `Committed` 才是后端插入成功；`OutcomeUnknown` 必须保持不确定，不重放相同文本；确定失败才按配置走 SendInput/clipboard fallback。 | submit ack、query complete/unknown、target PID/TID、最终 `InsertOutcome`。 |
| N6 背压 | realtime audio callback 不等待磁盘或慢消费者；有界队列达到策略上限时按设计丢归档或停止 capture；事件 lag 通过 replay/snapshot 恢复。 | queue-full/lagged 记录、各线程栈、归档文件与 Core snapshot。 |
| N7 可观测性 | 日志静默只能表示观测通道没有继续产生可见记录，不能单独表示进程退出、主线程冻结或所有业务线程停止。 | 进程存活、UI 消息循环、线程栈、文件写入状态和独立 dump。 |

## 4. 证据语义分类

为避免把“源码可能等待”误写成“现场已死锁”，本报告使用以下四种标签：

- **`VIOLATION`**：有现场运行证据直接违反 N 合约，例如两份 dump 显示主线程连续卡在同一不可恢复等待，且等待链可闭合；或 `OutcomeUnknown` 后出现同一请求的重复重放。
- **`SUSPICIOUS`**：源码存在长等待、串行化、迟到回调或资源 join 等风险，但没有现场栈/日志链证明它发生在本次故障。
- **`NO_VIOLATION_OBSERVED`**：在本次静态检查的源码边界内没有看到所描述的违反，例如 realtime callback 没有阻塞式归档发送；这不是运行时“绝对不会发生”的保证。
- **`INSUFFICIENT_EVIDENCE`**：缺少原始日志、dump、线程栈、符号匹配或稳定复现，无法把候选解释升级为现场结论。

Issue 摘录的语义复核（本轮没有取得或重新统计原始日志）：

- Issue L19228–L19236 表示录音已收到 PCM，随后仍有另一条快捷键警告；支持“有线程当时仍产生日志”，不支持录音开始瞬间所有线程已冻结。之后静默和后来启动，只构成 N4/N7 的覆盖缺口。
- Issue 记载的 12 次 TSF 超时后均有 `Inserted`，反对“这些请求因该次超时永久停住”的解释；后端终态并不验证目标文字，用户可见结果仍为 `INSUFFICIENT_EVIDENCE`。
- SwitchStyle 反复注册失败是已报告的异常，期间仍有业务完成；它与 N3 的运行状态有关，但与全 UI 卡死的因果关系尚未建立。

当前主要结果：

| 主题 | 静态结论 | 语义 |
|---|---|---|
| #1266 的具体根因 | 没有现场线程栈、等待链或原始日志，不能定位。 | `INSUFFICIENT_EVIDENCE` |
| `settings_host_gate` 直接死锁 | 找到共享 gate 和多种嵌套等待，但没有看到 `apply_hotkey_runtime_change` 重新获取该 gate 的直接反向边。 | `SUSPICIOUS`，不是 `VIOLATION` |
| 5 秒 ack 等待无限阻塞 | 相关 supervisor 的调用线程有 `recv_timeout(5s)`。 | 静态 `NO_VIOLATION_OBSERVED`，但只限该等待 |
| 热键 bridge 被业务流程占住 | Toggle 路径明确对完整 async handler `block_on`。 | `SUSPICIOUS` |
| realtime callback 等 WAV 磁盘 | archive 使用 `try_send`，满时丢 archive PCM。 | `NO_VIOLATION_OBSERVED` |
| stop/join 延迟 | `finish` 会发送 Finish 并 join writer/processor；没有全局业务 deadline。 | `SUSPICIOUS` |
| IME 永久等待 | 单次发现/发送/query 有预算；全链路和重复的业务 stop 没有总 deadline。 | 单次 `NO_VIOLATION_OBSERVED`，端到端 `SUSPICIOUS` |

## 5. `settings_host_gate`、监督循环与 5 秒 timeout

### 5.1 所有直接持有者

`Inner` 中的 `settings_host_gate` 是一个 `parking_lot::Mutex<()>`；字段定义见 [`coordinator.rs#L514-L526`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator.rs#L514-L526)。注释说明它要串行化“Core settings transaction + host effect”和 style-pack 删除效果；更新函数、锁方法以及不在 `apply_hotkey_runtime_change` 内重新取锁的位置见 [`coordinator.rs#L1642-L1711`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator.rs#L1642-L1711)。直接持有点如下：

1. 主热键 supervisor：[`hotkey_loops.rs#L87-L170`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L87-L170)。
2. QA supervisor：[`hotkey_loops.rs#L185-L295`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L185-L295)。
3. Selection-polish supervisor：[`hotkey_loops.rs#L320-L378`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L320-L378)。
4. Coding-agent supervisor：[`hotkey_loops.rs#L616-L750`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L616-L750)。
5. Dictation/combo supervisor：[`hotkey_loops.rs#L1398-L1441`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L1398-L1441)。
6. Translation supervisor：[`hotkey_loops.rs#L1492-L1582`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L1492-L1582)。
7. SwitchStyle/OpenApp/QuickNote action supervisor：[`hotkey_loops.rs#L1619-L1715`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L1619-L1715)。
8. Style-pack supervisor：[`hotkey_loops.rs#L1945-L2042`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L1945-L2042)。
9. 普通设置写入：`persist_settings_preserving_update_channel`、`persist_strict_settings`、更新频道、替换 dictation hotkey、字段持久化分别见 [`settings.rs#L191-L219`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/commands/settings.rs#L191-L219)、[`settings.rs#L572-L582`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/commands/settings.rs#L572-L582)、[`settings.rs#L845-L918`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/commands/settings.rs#L845-L918)。`set_settings` 还会在持久化后进行 voice activation、主线程托盘等后续副作用，入口见 [`settings.rs#L290-L353`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/commands/settings.rs#L290-L353)。
10. style-pack 删除命令：Core 删除已经提交后仍持有 host gate，再同步 hotkey effect，见 [`style_packs.rs#L166-L185`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/commands/style_packs.rs#L166-L185)。
11. restore runtime：`spawn_blocking` worker 获取相同 gate，应用平台配置、reconcile hotkeys，之后还有 remote input 和主线程 acknowledgement，见 [`restore_runtime.rs#L35-L72`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/restore_runtime.rs#L35-L72) 和 [`restore_runtime.rs#L87-L108`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/restore_runtime.rs#L87-L108)。

### 5.2 当前可证明的锁序与未闭合的循环

设置事务的可见顺序是：

```text
settings_host_gate
  -> Core::settings_write_gate
      -> SettingsRuntime::prepare/commit
          -> Coordinator::apply_hotkey_runtime_change
```

Core `update_settings` 在 [`api.rs#L6681-L6808`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/crates/openless-core/src/api.rs#L6681-L6808) 获取 `settings_write_gate`，执行 prepare/commit、持久化和失败恢复。Host `TauriSettingsRuntime::commit/restore` 在 [`settings.rs#L97-L166`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/commands/settings.rs#L97-L166) 调用 `apply_hotkey_runtime_change`。后者的实现从 [`coordinator.rs#L1654-L1707`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator.rs#L1654-L1707) 可见，没有重新获取 `settings_host_gate`。

所以，**当前源码没有证明**以下直接循环：

```text
settings_host_gate -> settings_write_gate -> settings_host_gate
```

这不是“没有任何死锁”的证明，仍有三类未由静态阅读排除的边：原生库内部等待、未纳入本次边界的其他锁、主线程闭包间接触发设置命令；还可能有 Tokio/TAURI worker starvation。应把它称为“静态锁序风险”，而不是已确认死锁。

### 5.3 5 秒 `recv_timeout` 究竟保护什么

`TauriCoordinatorHost::run_on_main_thread` 只是调用 Tauri `AppHandle::run_on_main_thread` 排队闭包并返回；host wrapper 自身没有 timeout，见 [`tauri_coordinator_host.rs#L2252-L2281`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/tauri_coordinator_host.rs#L2252-L2281)。具体 supervisor 把 ack 放入 channel，再由调用线程等待 5 秒：

- QA：闭包启动 `QaHotkeyMonitor` 后发送结果；代码和注释明确 `run_on_main_thread` 是 fire-and-forget、5 秒用于避免 supervisor 永久等待，见 [`hotkey_loops.rs#L233-L270`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L233-L270)。
- Selection-polish：主线程更新后通过 channel 回执，5 秒超时，见 [`hotkey_loops.rs#L365-L378`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L365-L378)。
- Coding-agent：主线程 combo registration 有 5 秒回执等待，见 [`hotkey_loops.rs#L665-L750`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L665-L750)。
- Translation、action、style-pack：分别在 [`hotkey_loops.rs#L1533-L1579`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L1533-L1579)、[`hotkey_loops.rs#L1659-L1699`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L1659-L1699)、[`hotkey_loops.rs#L2027-L2042`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L2027-L2042)。
- Native dictation binding：调用线程 5 秒后设置协作式 `cancelled`，如果主线程闭包还未开始，它会跳过注册；若闭包已经开始，则没有从调用线程强制中断的机制，见 [`native_dictation_key.rs#L16-L127`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/native_dictation_key.rs#L16-L127)。

**它只限制调用线程在 `recv_timeout` 中等待回执的时长；不是调用线程整轮执行或持锁时长的上限。它不保护：**

- action 等相关路径已经排进主线程队列的闭包不会因该 timeout 自动撤销（上面的 native dictation 路径另有协作式取消检查）；
- 闭包已经开始后的原生注册、`HotkeyMonitor`/`ComboHotkeyMonitor` 更新；
- `settings_host_gate` 本身的 `lock()` 获取；
- `ensure_runtime_ready()`、Core settings transaction、provider/ASR/LLM、录音停止、文本插入和 IME query；
- `host.block_on` 执行的 Pressed/Released 业务流程；
- 主热键 supervisor 的直接 `HotkeyMonitor::start`（该路径没有 5 秒回执 channel，见 [`hotkey_loops.rs#L87-L170`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L87-L170)）；
- fire-and-forget 的 removal/update 闭包。

因此“有 5 秒 timeout”不能简化为“所有热键注册最多阻塞 5 秒”，也不能简化为“5 秒后 UI 闭包一定停止”。

## 6. 热键事件、录音停止、插入与 IME

### 6.1 热键 bridge 是业务串行点，不是 UI 主线程

专用 `hotkey_bridge_loop` 从 channel 收事件。Toggle 模式的 Pressed/Released 通过 `inner.host.block_on` 串行运行完整 handler；Hold 模式把 handler `spawn` 到 runtime。源码注释明确 handler 会 await（包括网络握手），因此 bridge 线程可能在业务期间暂时不消费下一个事件，见 [`hotkey_loops.rs#L2178-L2271`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L2178-L2271)。combo bridge 具有同样的串行 Pressed/Released 设计，见 [`hotkey_loops.rs#L1444-L1490`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs#L1444-L1490)。

`dispatch` 会先启动 Core，再调用 `dispatch_dictation_hotkey_edge`，见 [`dictation_core.rs#L47-L55`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/coordinator/dictation_core.rs#L47-L55)。这可以解释“按键事件到了，但专用 bridge 长时间忙/后续 Release 延后消费”。它本身不能证明 Tauri 主消息循环停止；要证明全 UI 无响应，必须看到 UI 主线程或 WebView2/窗口消息线程的栈和等待链。

### 6.2 stop/finalize 没有全链路总 deadline

Core stop 在 [`api.rs#L8047-L8173`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/crates/openless-core/src/api.rs#L8047-L8173) 处理 Starting/Recording 状态；Starting 可能等待 recorder phase 改变，Recording 后切换到 Transcribing，再等待 `engine.finish`（ASR/LLM/其他 provider 业务）。后续会进入 Inserting，并在 [`api.rs#L8392-L8447`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/crates/openless-core/src/api.rs#L8392-L8447) 等待 `finish_text_insertion`。这条 stop/finalize 链没有覆盖 ASR、polish、录音关闭和 insertion 的统一总 timeout。

录音对象的 `stop` 把 recorder/warm capture 的停止放入 `spawn_blocking`，随后 await join；必要时还会 await voice activation reconfigure，见 [`core_adapters.rs#L3479-L3539`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/core_adapters.rs#L3479-L3539)。这是业务/阻塞线程等待，不是显式 UI main-thread callback。

### 6.3 TSF composition、确定失败与未知结果

Windows insertion 的 composition cleanup 最多三次、间隔 25 ms，composition update 会区分 `NotDispatched`、确定失败、冲突和未知结果，见 [`core_adapters.rs#L4388-L4551`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/core_adapters.rs#L4388-L4551)。最终插入路径对 live composition 先 Finish；无文本时 Cancel；TSF 确定失败才按配置选择 Unicode SendInput 或 clipboard/paste fallback；plain TSF 的 `OutcomeUnknown` 直接返回 `BackendErrorCode::OutcomeUnknown`，不重放文本，见 [`core_adapters.rs#L4635-L4803`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/core_adapters.rs#L4635-L4803)。clipboard fallback 也通过 blocking pool 执行，见 [`core_adapters.rs#L4832-L4843`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/core_adapters.rs#L4832-L4843)。

`WindowsImeSessionController` 只有 `Committed` 映射为 `InsertStatus::Inserted`；拒绝/失败是 Failed，`OutcomeUnknown` 由错误独立传播，见 [`windows_ime_session.rs#L53-L128`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/windows_ime_session.rs#L53-L128)。因此日志或状态中的 `Inserted` 只能说明该后端收到 committed 状态；它不证明目标应用的用户可见文字已经正确出现，也不能替代 UI/目标线程证据。

### 6.4 IME IPC 的同步、异步与时间边界

IME 常量为：窗口发现约 700 ms，单次 `SendMessageTimeoutW` 约 2 s，post-dispatch query 总预算约 5 s，query 间隔 10 ms，见 [`windows_ime_ipc.rs#L12-L23`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/windows_ime_ipc.rs#L12-L23)。

- `submit_text_to_platform` 把 Windows message send 和 polling 放入 `spawn_blocking`，不直接占用普通 async worker，见 [`windows_ime_ipc.rs#L316-L327`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/windows_ime_ipc.rs#L316-L327)。
- submit 先做目标窗口发现，再用 `SendMessageTimeoutW` 等 acknowledgement；ack 后每 10 ms query，直到 committed/确定状态或 5 秒 deadline，见 [`windows_ime_ipc.rs#L450-L591`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/windows_ime_ipc.rs#L450-L591)。
- 窗口发现/协议 probe 有自己的约 700 ms 总预算，优先精确 target thread，见 [`windows_ime_ipc.rs#L594-L687`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/windows_ime_ipc.rs#L594-L687)。
- ack 超时、窗口在 dispatch 后消失或 post-dispatch deadline 到期都可能是 `OutcomeUnknown`；它只证明调用者无法确认，不证明目标一定没有写入。

因此 IME 可以合理解释“业务 finish 等待数秒或返回未知”，但仅凭这些常量不能解释“整个应用主 UI 无响应”。还需现场线程栈确认 blocking worker 是否真的卡在 `SendMessageTimeoutW`/query，以及主线程是否同时被同一等待链牵连。

## 7. WAV、音频处理、事件和日志背压

### 7.1 音频 realtime callback 与处理队列

音频处理队列有 3 秒 queued duration、8 MiB queued bytes、4096 条 channel capacity；backlog 达到 500 ms 时先关闭可选 enhancement，常量见 [`recorder.rs#L34-L44`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/recorder.rs#L34-L44)。cpal callback 使用 `frame_tx.try_send`；队列满或 worker 断开时将 `accepting_audio` 置 false、发送 `EngineFailed` 并设置 stop flag，而不是等待慢 worker，见 [`recorder.rs#L2256-L2299`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/recorder.rs#L2256-L2299)。处理 worker 消费 frame，backlog 过高时关闭 enhancement；Shutdown 会 finish 后退出，见 [`recorder.rs#L2480-L2527`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/recorder.rs#L2480-L2527)。

所以源码支持“处理过载会让本次 capture 失败/停止”，该投递点不会等待处理队列腾出空间；但不能扩大为“整个 cpal callback 不会阻塞”。回调仍有分配、锁及可能的日志路径，须结合现场栈判断。处理器 `finish` 发送 Shutdown 并 join worker，见 [`recorder.rs#L2530-L2541`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/recorder.rs#L2530-L2541)，这可能让 stop 调用者等待已接受的 frame 排空。

### 7.2 WAV archive

WAV archive writer 在独立线程，容量 256。callback/下游只复制 PCM 并 `try_send`；队列满时首次告警并丢 archive PCM，ASR 仍收到完整 PCM，见 [`recorder.rs#L211-L253`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/recorder.rs#L211-L253)。停止时 `send(Finish)` 后 join writer，已接受的 archive 消息要刷完，见 [`recorder.rs#L255-L262`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/recorder.rs#L255-L262)。

这形成清晰边界：

- **归档 PCM 投递没有等待 writer 消费**：`try_send` 满即丢；这仅说明队列策略。`append` 的首次满队列/断连告警仍会调用 §7.4 的同步 logger，不能据此保证整条调用链没有同步 I/O。
- **存在静态业务延迟风险**：stop 路径的 `send(Finish)` 和 writer `join` 可能等待文件 I/O；如果 archive writer 或文件系统挂住，可能拖长 recorder stop。
- 该 join 仍不是 UI main-thread 卡死的直接证明；需要 dump 看 join 的调用线程和 UI 主线程是否相互等待。

### 7.3 Core EventBus

EventBus replay capacity 为 2048；`publish` 在 backlog Mutex 临界区内分配 sequence、更新 snapshot、维护 replay 并调用同步 `broadcast::Sender::send`，但注释明确临界区不能做异步工作或 Host callback，见 [`events.rs#L461-L554`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/crates/openless-core/src/events.rs#L461-L554)。订阅落后时应通过 `Lagged`、replay 和最新 snapshot resync；队列 lag 是丢失/重同步语义，不是直接的 UI deadlock 证据。

### 7.4 文件日志与“日志静默”

Tauri 使用 `CombinedLogger` 同时挂 `TermLogger` 和文件 `WriteLogger`，文件以 append 方式打开，见 [`lib.rs#L1872-L1913`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/lib.rs#L1872-L1913)。simplelog 的 `WriteLogger` 是同步写入实现（[现场依赖对应的 writelog.rs](https://github.com/Drakulix/simplelog.rs/blob/47f02be24dd3899fefcfdb027a3a67ad866ea925/src/loggers/writelog.rs#L68-L82)），CombinedLogger 顺序调用各 logger（[comblog.rs](https://github.com/Drakulix/simplelog.rs/blob/47f02be24dd3899fefcfdb027a3a67ad866ea925/src/loggers/comblog.rs#L92-L108)）。因此磁盘/锁争用可以拖慢产生日志的线程，也会造成日志停止；但没有现场线程栈时，不能把它定为主线程冻结原因。

源码另有独立、有界的 voice diagnostics NDJSON writer：producer 使用 `try_send`，满时计数丢弃，不让音频/模型工作等待磁盘，见 [`recorder.rs#L58-L113`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/recorder.rs#L58-L113)。本轮没有创建 debug 日志或追加探针，也没有把该既有 diagnostics 文件当作 #1266 的现场证据。

## 8. 可能的故障解释与不能越界的地方

| 现象/日志信号 | 源码能合理解释的范围 | 不能据此断言 |
|---|---|---|
| 日志在某条记录后静默 | 进程退出、主线程/业务 worker 等待、同步日志 I/O、日志文件不可写或日志路径不再可见都可能造成静默。 | 最后一条日志所在函数就是根因；所有线程都停止。 |
| 热键按下无反应 | supervisor 在注册/回执等待，或专用 bridge 被 Pressed/Released 的业务流程占用。 | Tauri UI 主线程必然卡死。 |
| Released 后录音不停止 | Starting/recorder stop/ASR/LLM/insert 链任一阶段等待；bridge 也可能尚未消费 Released。 | 一定是死锁；一定是 recorder callback 卡住。 |
| 后端出现 `Inserted` | TSF/IME 返回 `Committed`，或 Core 收到成功的后端 insert status。 | 目标窗口已向用户显示正确文字。 |
| TSF 出现超时/`OutcomeUnknown` | submit 已 dispatch 但 ack/query 不确定；保守策略不重放。 | 文本肯定没有写入；可以安全重发。 |
| 重启后恢复 | 可清理进程内 bridge、热键注册、TSF composition、IME IPC/native handle 等状态。 | 更新一定引入了同一根因；重启本身证明 deadlock。 |

### 8.1 关联报告的实际边界

- [#872](https://github.com/Open-Less/openless/issues/872) 仍 OPEN；其 macOS `sample` 报告是 bridge 等待而 main/tap 正常。这是区分业务线程与 UI 主线程的参考，不是 #1266 的根因证明。
- [#954](https://github.com/Open-Less/openless/issues/954) 的旧 Windows/TSF 多进程 AppHang 已 CLOSED，但[维护评论](https://github.com/Open-Less/openless/issues/954#issuecomment-5386351878)明确当时无法定位，[最终评论](https://github.com/Open-Less/openless/issues/954#issuecomment-5616700009)为之后出现再开；不能写成“已有确认修复”。
- [#1227](https://github.com/Open-Less/openless/issues/1227) 与 [#1261](https://github.com/Open-Less/openless/issues/1261) 有原生异常码/转储，不能因同样缺少 panic 日志便与 #1266 合并。尤其 [#1261 后续补充](https://github.com/Open-Less/openless/issues/1261#issuecomment-6382195634)还报告了 `insert_final` 阶段退出，不能把其全部现场归为“只在录音回调中崩溃”。
- #1261 的[诊断 run 40105276600](https://github.com/HKLHaoBin/openless/actions/runs/40105276600)虽已成功，head 是 `60155b000f754c69fe6f4e29ba3930d6bda948d2`，不是本 Issue 已验证的修复版。不得用其他 Issue 的诊断包替代 #1266 的因果验证。

### 8.2 先排除不同故障类型

H1–H5 仅排列源码候选，不能覆盖或排除所有故障类型。先记录同一 PID 是否还在、是否由用户结束、是否发生休眠/退出；若进程已结束，应转到退出/原生异常取证，不继续宣称主线程死锁。若主进程和后台业务仍有进展、只有 WebView2 窗口失响应，应单独保留对应子进程现场。

现场版本的 [`single_instance_callback`](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/lib.rs#L794-L815) 在 `start_minimized=true` 且无 startup error 时明确跳过再次显示窗口；这能解释部分“重复打开没窗口”，不是全应用卡死的通用解释。[托盘入口](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/lib.rs#L997-L1017)和[主循环事件](https://github.com/Open-Less/openless/blob/af14756d3d9d1474fa7db782c332b61deef58688/openless-all/app/src-tauri/src/lib.rs#L1055-L1100)可以帮助记录响应边界，但托盘 handler 自身也调用业务逻辑，托盘不响应仍不是主消息循环永久停止的独立证明。

## 9. 可证伪假设与现场探针

以下是假设，不是根因结论；对本现场的因果分类全部为 `INCONCLUSIVE`，目前没有可据以修复行为的 `ACTIONABLE` 假设。表中改动仅是若获得因果证据后的实验方向，每轮只能选一个确认边界、改变一个变量，不能一次实施整组架构改造。P1–P6 是只读取证观察点，不是已新增的运行时探针；本轮没有创建 debug 日志，也不要求用户安装未构建源码。若以后确需新增运行时诊断，须另行明确 2–6 个安全边界、会话专属 NDJSON 路径，并先经 Actions 构建再交付。

| ID | Hypothesis | Falsifiable prediction | If true, what change would make the bug disappear? | Probe IDs |
|---|---|---|---|---|
| H1 | 主线程被热键注册/设置副作用或原生回调牵连，`settings_host_gate` 的长持有使设置和 supervisor 相互等待。 | 卡住期间两份 dump 的 UI/main thread 在相同 `run_on_main_thread` 闭包、原生注册或锁等待；等待链能指出 gate owner；若主线程连续处理消息且 gate owner 在业务 worker，则 H1 被否定。 | 仅修正确认的那一处锁序或跨线程持锁等待，保留状态串行化与已有代次/取消保护；不整体删除 gate 或靠延时掩盖。 | P1, P2 |
| H2 | 热键专用 bridge 在 `block_on(handle_released_edge)` 内等待 stop/ASR/insertion，导致后续边沿不消费，而 UI 仍可能活着。 | bridge 线程连续两份 dump 都在 `hotkey_bridge_loop -> handle_released_edge -> dispatch -> stop...`；主消息循环/窗口响应仍有进展。若 bridge 空闲而 UI 主线程挂住，则 H2 被否定。 | 保留边沿 FIFO，但把长业务流程改成可观测、可取消的 session state machine；让 bridge 快速确认事件并由受控 worker 完成 stop。 | P1, P3 |
| H3 | TSF/IME 目标线程或 OpenLess IME DLL 在 submit ack/query 阶段等待，拖住 insertion/finish。 | blocking worker 栈位于 `SendMessageTimeoutW`、query 或 `find_ime_window_with_retry`；日志有 submit begin/ack 但没有 complete，且 target PID/TID 与现场窗口一致。若 blocking worker 不在此路径，则 H3 被否定。 | 将 IPC request 与业务 finalize 隔离，超时后 fail-fast；修复目标线程/IME DLL 协议或句柄生命周期；保持 `OutcomeUnknown` 不重放。 | P1, P4 |
| H4 | recorder stop 的 audio processor/WAV archive join 或文件 I/O 使业务停止变慢。 | 非 UI 线程栈停在 archive writer `join`、文件写入或 audio processor `join`；能看到 archive backlog/处理过载，主 UI thread 没有同一等待链。若 writer 已退出且 stack 在 ASR/IME，则 H4 被否定。 | 修正确认的 join/资源生命周期边界，保留已接收音频的收尾、取消语义和资源释放；不强制遗弃 worker 或静默丢弃必要数据。 | P1, P5 |
| H5 | 同步 `WriteLogger`/文件系统 I/O 造成日志生产线程停滞，日志静默掩盖了真实业务状态。 | 线程栈在 simplelog `WriteLogger`/`write_all` 或文件锁；UI/业务状态仍继续，或仅日志线程停止。若所有线程都在其他等待链，则 H5 被否定。 | 采用异步、有界、可丢弃的普通日志 sink；限制单条日志和 flush 影响；保留独立 crash/hang dump 采集。 | P1, P6 |

### 探针定义（只提出）

- **P1：同一 PID 的双份 hang dump。** 卡住但进程尚未退出时，用 [Microsoft ProcDump](https://learn.microsoft.com/en-us/sysinternals/downloads/procdump) 的 `-ma PID` 取 full dump；间隔约 10 秒再取一份。这个间隔只是为了判断栈是否前进，不是产品延迟阈值。两份都分析全部线程，优先区分 UI/main、bridge、Tokio worker、blocking pool、audio/archive、IME/DLL 线程。
- **P2：等待链与主消息循环证据。** 对 P1 的线程使用 [Microsoft Wait Chain Traversal](https://learn.microsoft.com/en-us/windows/win32/debug/wait-chain-traversal) 或调试器检查 ALPC/COM/critical section/mutex/SendMessage/process-thread 等等待，并记录 main thread 是否仍能处理窗口消息。WCT 没有 Rust/Tokio future 的完整语义，不能单独排除异步等待。
- **P3：事件到 Core phase 的时间线。** 从现有热键/听写/Core 状态记录中按 session/press id 对齐 Pressed、Released、Starting、Recording、Transcribing、Inserting、Completed/Failed；不向共享日志追加大量探针。重点验证 Released 是否已经进入 bridge、是否被业务流程延后消费。
- **P4：IME target/协议链采样。** 将现有 submit begin、ack、query complete/unknown 与 target PID/TID、IME PID/TID 对齐，并结合 P1 栈确认 `SendMessageTimeoutW`/query 的调用线程；不要把 ack 当作用户可见提交。
- **P5：停止阶段资源链采样。** 结合 recorder 现有 queue-full/processor/archive 状态和 P1 栈，判断停止时是否在 audio processor/WAV writer join 或文件 I/O；区分 realtime callback、stop worker 和 UI/main thread。
- **P6：日志通道对照。** 对照进程存活、UI/托盘响应时间、文件句柄/文件大小变化和 P1 栈，判断是同步日志写入停滞还是业务本身停止；不要用向同一同步日志通道继续写更多记录来证明日志通道正常。

## 10. 最小下一步

1. **优先在下一次复现时保留现场。** 不要先重启；确认进程 PID 仍存在后按 P1 取两份 full dump，并保存采集时刻、前台窗口、是否能响应托盘/窗口操作。
2. **用匹配符号分析全部线程。** 先取得现场 EXE SHA-256，再核对 EXE 的 PE debug directory 与 PDB GUID+Age；仅使用与 `af14756d...` 对应的 EXE/PDB，不把 artifact 存在误写成现场匹配。符号说明见 [Microsoft Symbols and Symbol Files](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/symbols-and-symbol-files)。
3. **按 P2 分层判定。** 先回答“UI/main thread 是否停在等待/原生调用”，再回答“bridge、IME、recorder、archive、Tokio worker 哪一条业务链变慢”；不要从最后日志行反推。
4. **若 main thread 没有卡住，按 P3–P5 检查业务阻塞。** 重点是 Released 消费、stop/finalize phase、IME target thread 和 recorder/archive join；若只有日志停滞，再按 P6 检查日志 I/O。
5. **取证后再选一项最小实验。** 若确认 H1，仅调整被证实的跨线程持锁等待或锁序；H2–H5 同理针对确认边界，不一并改热键、音频设备、ASR 和输入模式。保留物理边沿顺序、代次/取消、IME 不确定结果不重放及资源清理。
6. **保留隐私和取样边界。** dump 可能包含凭据、录音、识别文本和其他应用内容，不上传公开 Issue；先协商受控传递方式，公开仅提供脱敏栈、阶段和时间线。ProcDump `-h` 的官方定义只是窗口至少 5 秒不响应消息，不覆盖所有后台业务挂起，`-e` 的异常触发也不能代替挂起取样。手动 `-ma PID` 不依赖 WER 自动崩溃报告；当前尚未实际采集或分析 dump。
7. **本轮停止点。** 缺少 #1266 自己的挂起现场及可验证复现，不能进入修复验收；本轮完成的是调研。以后若修改代码，CI、桌面和 Android 构建须针对实际分支及准确提交在 GitHub Actions 执行，优先交付桌面产物。本轮没有修改或推送产品代码，因此不触发新构建。

## 11. 最终结论

截至现场提交 `af14756d...` 的源码审计，#1266 的“更新后无响应”仍是 `INSUFFICIENT_EVIDENCE`。源码确实存在可能造成**热键业务路径长时间不前进**、**设置/注册收敛延迟**和**停止/插入流程等待**的结构；同时也明确提供了若干边界：多数主线程注册 ack 等待为 5 秒、IME 单次发现/发送/query 有预算、音频/WAV realtime 投递不等待慢消费者、EventBus 不在 publish 临界区调用 Host callback。

这些边界不能合并成“全 UI 一定不会卡死”，也不能把任何一条可疑路径直接称为现场根因。下一步优先取得同一 PID 尚未结束时的双份 hang dump、全线程栈/等待链及与现场 EXE 匹配的符号，以区分故障层次；它们不是充分的因果证明，也不是唯一证据形式。只有能解释原始症状、排除竞争解释的现场链或受控实验，才能支持具体归因和修复尝试。
