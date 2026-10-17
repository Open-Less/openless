# macOS 语音链路只读审查

审查仓库：`/Users/trip/orca/workspaces/openless/修复-Matel存在的部分问题`。原始审查 HEAD：`0947c05a5b911aebce7a750c3045160859428814`；PR 范围 `0454dc56..0947c05a`。以下原始问题行号以该 HEAD 为准；末尾另评主 Agent 尚未提交的修复。时间均明确标注 UTC 或 +0800。

## 结论

**P1：已定位与本机配置及最新日志吻合的首录音永久 Processing 根因。** 显式指定麦克风名称时，CPAL 0.15.3 的 macOS disconnect listener 形成强引用环，音频回调继续持有队列 sender；OpenLess 新增音频处理 worker 却依赖所有 sender 析构才退出，停止录音会永久卡在 join。此缺陷由 `44972015`（#1194）引入，已经存在于审查基线，不能归入本次 UI/实验 ASR 的新增回归。

另确认两个独立缺陷：Qwen/Stepfun Realtime 启动错误清理自锁（历史问题），以及 TextStream 等待进程退出时丢失可取消句柄（`16a73b9d` 新增）。当前本机偏好使用 DashScope 多模态，实验模型全部关闭，后两者不解释最新故障窗口。

## 1. P1：指定麦克风的回调 sender 被 CPAL 引用环保留，停止时永远等待 EOF

**主定位**：`openless-all/app/src-tauri/src/recorder.rs:1515`、`:1871`、`:1906`。

**触发条件**：macOS，选择一个非空名称的麦克风（即使名称恰好是当前系统默认内建麦克风），开始并停止普通听写、设置页电平预览或其他共用 Recorder 的会话。无需 ASR 服务报错，分步与多模态均受影响。

可复核因果链：

1. `recorder.rs:1676-1682` 按名称从 `host.input_devices()` 返回设备。锁定依赖为 `openless-all/app/src-tauri/Cargo.lock:1065-1066` 的 CPAL 0.15.3。
2. 本机 CPAL 源码 `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cpal-0.15.3/src/host/coreaudio/macos/enumerate.rs:84-86` 将枚举设备标记为 `is_default=false`；只有 `default_input_device` 创建的 handle 标记为 true（同文件 `:114-116`）。
3. 该依赖 `src/host/coreaudio/macos/mod.rs:616-617` 为非 default handle 安装 disconnect listener；`:456-468` 的 listener closure 捕获 `stream.clone()`，而 StreamInner 又拥有 listener，形成 `StreamInner -> listener -> Stream(Arc<StreamInner>)` 强引用环。
4. OpenLess 回调在 `recorder.rs:1780-1789` 捕获 `frame_tx`。`drop(stream)` 不能解开依赖的环，因此回调及该 sender 并未析构。`:1512` 的 `cpal Stream dropped (mic released)` 只说明外部 stream handle 被 drop，不能证明所有回调资源或处理线程已结束。
5. `:1513-1514` 只释放 OpenLess 自己的 sender；worker 在 `:1871` 等待 `receiver.recv()` 返回 disconnected，`:1904-1906` 无退出指令地 join，永远无法返回。
6. `core_adapters.rs:2502-2514` 的停止任务因此无法返回；`crates/openless-core/src/cloud_providers.rs:2191-2198` 的 Omni finish 要先等待录音 stop，后面才调用多模态请求。用户能看到 Processing，但请求尚未发出。

**无电平的相关扩散路径**：`src-tauri/src/commands/misc.rs:199-205` 停止预览时持有 `MicrophoneMonitorState.operation` 并同步执行 `recorder.stop()`；卡住后再次预览和普通听写都会等这把锁。`src-tauri/src/core_adapters.rs:2648-2653` 在异步任务中同步取得该锁，或直接 `preview.stop()`，此时新 Recorder 尚未启动，因而可能表现为无电平并占住 Tokio worker。该路径是源码确认的后续影响；未通过真人操作确认用户具体经历过哪次预览交接。

**引入记录**：`git blame` 将 `processor.finish()`、worker 的 `recv`/`join` 指向 `44972015`，提交时间 `2026-10-14 17:51:39 +0800`，`feat(audio): add lightweight microphone noise suppression (#1194)`。`git merge-base --is-ancestor 44972015 0454dc56` 成功；基线提交时间是 17:51:53，故它在基线前。按名称选设备代码更早来自 `ec773f08`（#306）；引用环本身位于锁定第三方依赖。

**修复建议**：worker 应拥有显式、FIFO 的 Shutdown 结束协议，在停止接收原生帧后排空已入队帧并退出，不能把 native callback 析构当作 worker 结束条件。覆盖“仍保留一份 native sender、队列已有 PCM 时，stop 有界完成且已入队帧不丢”的纯内存回归；同时覆盖设置预览停止再开始听写。不要靠丢弃 join 或提前释放 voice gate 绕过清理。

**验证**：已读锁定 CPAL 实际源码，并用标准库 Rust 2021 探针模拟相同的 Arc/closure/sender 所有权结构。drop 外部 Stream 和 sender 后 worker 100ms 内仍等待；断开模型中的引用环后立即结束。另探针确认保留 native sender 时 FIFO Shutdown 仍能排空帧并退出。未实际打开麦克风或调用 CoreAudio；因此是源码、隔离复现和本机条件吻合的高可信根因，非真实设备端到端实测结论。

## 2. P1（历史）：Qwen/Stepfun Realtime 启动失败时重入同一非重入锁

**定位**：`crates/openless-core/src/asr/qwen_realtime.rs:307-308`，其 `cancel()` 再锁于 `:375`；同类代码 `asr/stepfun_realtime.rs:375-376`，再锁于 `:530`。这些路径均位于 `openless-all/app/` 下。

**触发条件**：对应 provider 在 `session.updated` 前收到服务端 error/连接关闭等启动失败。`if let Some(error) = self.state.lock().session_start_error.clone()` 的临时 guard 在成功匹配分支仍存活；分支中 `self.cancel()` 再次锁定 state，自锁。网络超时不能解除一个同步 Mutex 阻塞。稳定转写在停止后启动 ASR，因此可表现为永久 Processing；非稳定模式卡 Starting，stop 等待启动完成。

**引入记录**：Qwen 首次为 `9ed34dab`（2026-07-12，#775）；Stepfun 为 `17fe67c5`（2026-07-18，#835）。二者均早于本审查基线，当前本机多模态路径不经过它们。

**修复建议**：先在独立语句/作用域复制 `session_start_error`，释放 guard 后再判断并 cancel；用假 WebSocket/无网络状态机回归覆盖启动前错误和关闭，确认返回错误、释放会话而非挂住。

**验证**：无依赖 Rust 2021 探针在 if-let 分支中 `try_lock` 得到 WouldBlock，独立 snapshot 后能够取得锁；与仓库 `edition=2021` 一致。未运行真实 provider 请求。最新日志无该 provider 的直接命中证据，不能当作本次用户故障原因。

## 3. P2（本分支新增）：TextStream 完成等待取走 Child，超时/取消无法终止仍存活进程

**定位**：`openless-all/app/src-tauri/src/asr/local/experimental/textstream.rs:57-64`，调用于 `:367-368`，取消路径 `:45-55`、`:443-455`。

**触发条件**：worker stdout EOF，但 worker 进程尚未退出（例如原生库或解释器退出阶段仍运行）。`completed()` 先从 `Mutex<Option<Child>>` take 句柄，再在局部变量中 `wait()`；并发 `stop()` 见 None 即返回。180 秒 finalization timeout、Esc、切换/关闭渠道均可能报告清理结束，而本地进程仍存活；`running()` 也会误认无进程。不是当前全部关闭的实验模型所致。

**引入记录**：`16a73b9d`（2026-10-15，实验 ASR 提交）首次新增，属于 `0454dc56..HEAD`。

**修复建议**：在完成等待期间保留统一可终止的进程所有权，例如短锁 `try_wait` 轮询，由 stop 独占取走并 kill/wait；不要在持全局 mutex 的阻塞 wait 中等待退出，也不要先取走 Child 使取消失去目标。测试 EOF-before-exit 与取消/超时并发，断言子进程已真正被回收。

**验证**：从仓库提取原始 `Process` 实现放入临时独立 Rust 探针，以仅关闭 stdout 后等待的 shell 子进程模拟 worker。已验证 completed 取走句柄后 stop 返回但 child 仍存活；探针自行 kill 并 reap 其唯一测试进程。未运行 Python 模型、SSE 服务、下载或真实推理。

## 本机证据与排除项

协调 Agent 追加授权后，只对白名单非秘密偏好字段做只读检查，未读 Keychain。`preferences.json` mtime 为 `2026-10-15 20:25:53 +0800`：`pipelineMode=multimodal`、`multimodalPipelineEnabled=true`、`activeOmniProvider=dashscope-omni`、`stableTranscriptionEnabled=false`、`microphoneDeviceName=MacBook Air麦克风`、Fn toggle、`lowLatencyDictationEnabled=false`、`audioEnhancementEnabled=true`、静音自动停止和录音静音均关闭。`experimental-asr.json` 的三个 enabled 均 false。偏好快照不是每一历史会话的完整运行态证明，但其修改时间先于以下最新问题窗口。

只读检查 `/Users/trip/Library/Logs/OpenLess/openless.log`（当时末行 13439、mtime 20:38:43 +0800）与滚动日志；未保存正文、转写、请求内容或任何秘密。

| 原日志位置 | UTC 时间 | 可用事实 |
|---|---|---|
| `openless.log:13264-13273` | 12:25–12:26 | 麦克风权限探针均 granted |
| `openless.log:13274,13277,13278` | 12:26:44 → 12:26:49 | 首次 recording 胶囊、输入设备启动、Stream dropped；之后未见 Omni POST |
| `openless.log:13396,13399` | 12:37:00 | recording 胶囊与输入设备启动；退出前未见对应 stream drop |
| `openless.log:13434,13435` | 12:38:36 → 12:38:38 | 再次输入设备启动与 Stream dropped；仍无后续 Omni POST |
| `openless.log:12328,12329` | 00:07:47 | 一次明确 watchdog 回调停止 4 秒，随后释放原生流；此记录更早，不能证明最新会话也是硬件回调停顿 |

`recorder.rs:1655-1660` 在 `stream.play()` 成功后才打印 inputDevice，因此有此日志可证明 play 返回成功。`omni.rs:320-326` 在发送多模态请求前固定打印 `[omni] POST`，最新故障窗口中没有该标签，与 stop 卡在 processor join 相容。

旧 `cb#` 日志来自 `recorder.rs:1987` 一带的旧 `process_callback`；`44972015` 新 worker 在 `:1870-1884` 不打印它。**缺少 cb# 不是无电平或未采集的证据。** 同样，native Stream dropped 不等于 audio processor 已退出。

最新两份 `DiagnosticReports/OpenLess-2026-10-14-193439.ips`、`...193133.ips` 为旧 `0.1.0 (1)` 的 AppleSpeechService.permissions / Swift executor SIGTRAP，均在本次 10 月 15 日窗口之前；不应拿来解释当前多模态 Processing。没有导出诊断报告全文。

## 已检查但未发现本分支直接新增的阻塞

- 普通 hotkey bridge、PipelineDictationEngine、SharedOmniDictationEngine、Recorder、cloud ASR/Omni 主体在 `0454dc56..HEAD` 未改动；hotkey_loops 的差异仅测试开关。8c32db9e 对 Capsule 的相关改动为字体样式，没有改听写状态流转。
- macOS Fn system-defined 事件新增主线程同步分类，但 listener shutdown 不 join 主线程；本次未发现足以解释上述 stop 日志的新增循环等待。
- Core stop 在读取 Starting 前登记 phase_changed waiter，并绑定具体 session ID；现有实现能防止 stop-before-ready 的普通丢唤醒/停错会话。启动未完成时 stop 等待是明确行为；provider 或原生资源清理挂住时该等待不会自行解决。
- TauriTaskSpawner 明确使用 Tauri 共享 runtime；常规录音 start/stop 的原生重操作位于 spawn_blocking，未发现本分支引入新的私有 runtime 生命周期问题。预览交接中仍有同步 stop/锁进入异步任务的边界，已作为问题 1 的扩散路径说明。
- 凭据退役投影、实验注册和通道释放的变更已读；当前实验全部关闭且多模态不经过本地 ASR 启动。本次未发现这些新增操作能解释 recorder 已 stop 却无 Omni 请求的确定锁链。

## 主 Agent 修复 diff 的独立复核

已只读复核主 Agent 在本次审查期间对 `recorder.rs` 的未提交修改：增加 `AudioProcessingMessage::Shutdown`、worker 自持 shutdown sender、finish 发 FIFO Shutdown 后 join，以及 `audio_processing_shutdown_drains_frames_with_a_retained_native_sender`（8 帧、2 秒、保留 native sender）回归。该退出协议与问题 1 的确定阻塞条件吻合，未发现此 diff 新增问题；可以使 preview->dictation 的资源交接返回。依赖自身的 AudioUnit/closure 引用环仍是另一个原生资源释放边界，显式 Shutdown 修复的是等待这个环析构导致的应用死锁。

我未运行主 Agent 的仓库回归，避免重复构建；其编译/测试结果由主 Agent 汇总。本审查运行的三个隔离探针全部通过，临时源码、可执行文件与探针子进程已清理。没有改仓库源码、配置、凭据，未安装应用、创建子 Agent、主动录音、调用收费 API 或完整构建；仅新增本报告。

仍未验证：真实设备开始/停止及设置预览交接的端到端表现、固定后的请求和最终输入、其他麦克风设备/拔插、CPAL 原生资源环是否另需依赖修复、实验模型真实退出阶段。报告中所有根因与修复结论均限定于上述证据。
