# #1227 现场反馈与 PR #1267 对照

## 结论与交付边界

PR [#1267](https://github.com/Open-Less/openless/pull/1267) 不能作为 GDORT 本次问题已经解决的依据。
核对时 PR 为 OPEN，head `1f84fa590ece5bb25ad1d8ce9de0c441d3071e94`，仅修改
`tauri_coordinator_host.rs` 中 capsule hit-test subclass 的 HWND/predecessor 校验。
作者明确声明这是 robustness/correctness 修正，**不是 `0xc0000374` 堆损坏的修复**。
当前证据既没有证明本次发生 HWND recycling，也没有 PR 后同条件复测；不能因而断言绝无关联。

上游另外存在 [WASAPI 补丁 `f88bf246`](https://github.com/Open-Less/openless/commit/f88bf246e7b6823ecd0422475cca3802ed4a50bb)。
其 `vendor/cpal/OPENLESS_PATCH.md` 明确区分已识别 COM 生命周期缺陷与未证实的 #1227 因果关系；
该提交还改变 packet release、零帧处理和权限检查的重复开流行为，不能整体套用后称为单变量实验。
其 Windows CI 通过不是原机活跃录音的堆损坏复现；硬件依赖测试默认 ignored。

**本轮没有 ACTIONABLE 的行为修复。** 最小下一步是修正 P2 对实际胶囊 dispatcher 的覆盖，
记录真实的派发来源、执行线程和父 operation，并复用 P6 生命周期。
这是诊断覆盖修正，不是根因确认，也不称为崩溃修复或新 A1 行为实验。
若仍不能定位闭包内的失败边界，下一项必需证据是准确匹配 EXE/PDB 的故障栈/转储，
而不是不断扩大埋点、关闭增强或同时合入多个上游补丁。

## 证据身份与只读处理

- 反馈：[GDORT，2026-10-25](https://github.com/Open-Less/openless/issues/1227#issuecomment-6399836845)。
  报告环境为 Windows 10 19045、本地 ASR 0.6B、IME 插入；报告安装 run `40105276600` / `60155b00`。
  旧 NDJSON 不含绑定 EXE 的 build_id，不能声称已由日志独立验证完整二进制身份。
- 原附件：`openless-issue-1261-session.zip`，SHA-256
  `126f1c374bae822e6ef77508d1b48d574b15531092ced7a937baac25bb1737fa`。
  唯一 member 为 `openless-issue-1261-9e0ffd34-session.ndjson`，67,243,388 字节。
- 从隔离下载目录流式读取 ZIP member，以 `python -I` 解析；不执行附件、不改写/截断/删除原日志，
  不把原始 NDJSON、ZIP、dump 或普通应用日志纳入仓库。
- 共 201,754 条记录，0 malformed，session 全为 `issue-1261-9e0ffd34`。
  以下 L 行号均为 **ZIP member 的原始全局行号**，不是解压后重新编号。
- 3 个 run 的已写 coverage 都是 dropped=0/io_failed=false；这不保证异常退出时最后队列已写完。
  文件行顺序存在并发 event_seq 逆序，不能当跨线程因果时钟；旧 schema 没有 parent/thread 身份。
- 下表只列安全聚合，不展示原始日志内容。未重新公开上传附件。

## 分运行重建

时间统一为 UTC+8，日期均为 2026-10-25。

| run_id | 原 member 行 | 首末时间 | 主线程 entered/returned | capture start/完整 close |
|---|---|---|---|---|
| r-18e1c99de8563444 | L1–56279 | 21:43:26.715–23:16:47.113 | 12956/12955 | 6/5 |
| r-18e1ceb8caa40244 | L56280–135773 | 23:16:59.739–23:37:33.763 | 18316/18316 | 5/5 |
| r-18e1cfda1fd41994 | L135774–201754 | 23:37:42.413–23:40:34.943 | 15203/15203 | 4/4 |

### Run1：报告的活跃录音堆损坏

- 最后 capture `op-12739`：audio_started L41300、configured L41301、first_output L41302；
  开始于 23:16:31.254，约 49ms 后首输出。没有该 capture 的 closing/drop/shutdown/join。
  活跃录音被 abrupt death 截断可能就是缺清理事件的原因，不能反推关闭路径导致崩溃。
- `op-17343` 是 **source=1 普通胶囊 dispatch**：L56244 requested、L56245 dispatch returned、
  L56246 task entered，均在 23:16:42.723；未见 task returned。
- L56247 layout_changed=false；其后 source=2 的 `op-17345`、`op-17346` 均进入并返回
  （L56248–56255，最后 task returned 为 L56254）。这不能证明它们就是 `op-17343` 的嵌套子调用。
- 之后 12 个成功派发没有 entry：8 个 source=1（23:16:42.733–.803），随后约 4.307 秒间隙，
  还有 source=3 和 3 个 source=1（23:16:47.110–.113，L56272–56279）。
  因而完整尾部距最后 task 层观察约 **4.390 秒**，不是只有 80ms。
- 已返回 task 的最大 duration_us=4926（L9907），不能据此给唯一未返回 task 一个 <10ms 上界。
  旧 safe.duration_us 是 operation 自开始的累计单调耗时，不是所有 stage 自身耗时。
- Event1000/`0xc0000374` 来自报告者的系统事件观察，不是 NDJSON 的异常记录。
  堆检测点可能晚于首次非法写入；未返回 task 可是原因、受害者或进程失败后的在飞工作。

### Run2：报告的静默消失

- 实际有 **5 次** capture，不是反馈文字中的 4 次。最后 `op-21631` 在 L126452 开始，
  23:37:20.151；关闭链至 L135600，23:37:27.241。后续主线程记录持续到 23:37:33.763。
- 全部 dispatch/entry/return 配对，只能说明已记录任务未留下不配对；不证明进程正常退出。
- 旧版本没有 Ready/CloseRequested/Destroyed/ExitRequested/Exit 覆盖；无 WER 也只是外部观察，
  不能证明无原生异常或同 #1266 同因。需分别反馈真实进程状态与退出生命周期。

### 共同覆盖限制

15 次配置均为 48k/mono/float，增强初始化启用；现有 worker 可因 backlog 自动关增强，
所以配置位 **不证明崩溃时仍实际执行增强**。mac_type_present 均 false 削弱“必须有 MacType 才崩”的说法，
不排除其他注入/驱动。没有任何 P4，不证明 IME 无关或报告者没有选择 IME。
仅参数和首输出也不足以验证 16k/mono/i16 输出内容与用户可见插入结果。

## 正常行为与语义分类

| ID | 期望与来源 | 现有证据分类及最小下一步 |
|---|---|---|
| N0 | 普通录音/识别/插入不得使进程死亡；用户结果契约 | Run1 报告者外部观察 VIOLATION；最小下一步为准确包的匹配故障证据，日志本身不包含异常栈 |
| N1 | Tauri dispatcher 的提交返回不等于任务完成；真实闭包源码契约 | Run1 P3/source=1 L56244–56279 SUSPICIOUS；补实际 capsule dispatcher 的线程/parent 以区分排队与重入 |
| N2 | 正常 stop 保持停止接收、pause/drop、drain、join；recorder 源码契约 | 14 次已记录正常关闭 NO_VIOLATION_OBSERVED；Run1 活跃 capture 未开始 stop 不是清理顺序违例 |
| N3 | 16k/mono/i16 输出、原取消和完整收尾；AudioConsumer/recorder 契约 | INSUFFICIENT_EVIDENCE；参数/首输出不能代替用户可见结果或输入输出验证 |
| N4 | 将静默消失与正常退出、强制终止及 UI-only 假死区分；用户结果契约 | Run2 INSUFFICIENT_EVIDENCE；使用已有 P6 加外部进程/响应观察 |
| N5 | IME 未知结果不重放，后端终态不冒充文档已插入；IME 源码契约 | INSUFFICIENT_EVIDENCE；无 P4，保持原插入方式并反馈实际可见结果 |

去掉所有 ERROR/WARN，Run1 的进程死亡结果仍违反 N0；未返回任务仍是 N1 异常线索。
但错误级别、缺清理、MacType 不在场都没有建立根因；Run2 的配对成功也不满足 N0/N4 的整体退出解释。

## 假设、因果信心与修复资格

`H1227-*` 是本轮编号，**不与 #1266 的 H1–H5 同义**。
各项当前均为 causal **INCONCLUSIVE**、fix eligibility **INCONCLUSIVE**，无 CONFIRMED/ACTIONABLE 项。

| ID | Hypothesis | Falsifiable prediction | If true, what change would make the bug disappear? | Probe IDs |
|---|---|---|---|---|
| H1227-1 | 胶囊 native/Wry 执行存在错误重入或失去推进（N0/N1） | 同线程及真实 parent 定位外层任务与子派发；仍需具体错误 native 边界，重入本身不是违例 | 仅修已证实的重入/生命周期边界，不丢 payload 或绕过取消 | P2；P6 区分终止 |
| H1227-2 | 活跃 WASAPI 服务所有者/生命周期错误（N0/N2） | 原机匹配栈/转储揭示具体 native 所有者违例或非法访问 | 仅修该项 ownership，不同时修改 packet/权限 | P5 只有 stop 背景；活跃 COM 覆盖缺失，需匹配 native 证据 |
| H1227-3 | 有效增强 DSP 造成内存破坏（N0/N3） | 故障在有效增强处理而非初始配置；同仪器对照仅改变增强位 | 仅禁用 worker 实际 enhancement，保持格式/重采样/Flush/清理 | P5 不覆盖活跃 DSP 内部；当前需匹配故障证据 |
| H1227-4 | Run2 走了错误的应用退出路径（N0/N4） | P6 真实 exit 请求及其具体非预期来源 | 仅 guard 该项错误退出请求，不吞所有 close/exit | P6 |

P2/P6 的修正不声称覆盖 H1227-2/3。单凭 unsafe、ntdll、所有 capture 都启用增强，
不能选择关闭 DSP 或改音频驱动。下一份日志若仍只能指向“闭包某处”，报告匹配 dump/栈缺口，不无限补点。

## 本轮实现与复测门禁

保持当前分支基线 `38fbe1c0`，只扩展诊断，不混入 #1267、WASAPI 或最新同步行为。
`60155b00` 与原 base 的相关 recorder/capsule 差异是旧探针接线；新包仍必须按自己的准确 SHA 验证。

- P2 两种 host adapter 使用同一 MainDispatch tracer 观察同一 Tauri 主线程队列 API：
  host/capsule/mirror/layout 四个闭合来源；实际 requested/dispatch_returned/entered/returned/scope_left。
- `parent_operation_id` 仅来自派发时该线程真实执行的 P2 scope；queued child 可以晚于 parent 离开，
  因而 parent 只证明派发来源，**不单独证明同步重入**。须同时核对线程、单调时间与 entry/return。
- `thread_label` 由运行内计数分配，是 producer 线程的 opaque 数字，不是 OS TID/PID、线程名或 writer 身份。
  返回和 unwind 恢复上下文；未执行的闭包不能产生 entered 或 scope_left。
- 仍为六类边界、同一日志、单 writer/512 try_send；不加 native-call stage 扫描、timer、sleep、retry 或新 UI。
  高频胶囊 dispatch 会增加记录量与观测开销；不能承诺它不影响复现。
- A0-observation/C0-no-behavior-change 保持不变；准确 build_id 区分探针修订。
- Actions 中执行引用生产 tracer 的内存 sink/假 dispatcher 测试，覆盖直接/入队/拒绝、嵌套、unwind、
  queued parent 及线程隔离；保留 sink/privacy/feature off-on 检查。契约不是现场复现。
- Bugbot 无有效发现后才提交推 fork；准确 SHA 的 CI、桌面和 Android 全部成功后给 Windows artifact，
  保留 EXE/PDB 配对与 provenance。不创建 PR/release，不公开上传现场信息。

**请你复现 bug**

1. 安装本轮准确 SHA 的已验证 Windows 诊断包，保存 EXE/PDB/manifest；保持原设备、ASR、IME、快捷键及使用方式。
2. 按原场景录音/识别/插入，分别观察活跃录音崩溃与静默消失；记录实际结果及使用时间，未复现也不要称修复。
3. 故障时先记录发生时间、进程是否存在、主窗口/胶囊/托盘/热键各自是否响应；保留现场后再重启。
4. 私密反馈 `%TEMP%\openless-issue-1266-2e2c7a47-session.ndjson`、可获得的 health 和外部结果；
   若崩溃，受控提供与该 EXE/PDB 匹配的 WER/dump。不要在公开 Issue 上传 dump、录音或完整应用日志。
5. 若证据使某项具体变更 ACTIONABLE，在同一探针修订上只改该变量，分配 attempt/change，再经 Bugbot/Actions
   交付修复实验。现场同合同判 EFFECTIVE/INEFFECTIVE/INCONCLUSIVE；用户明确确认前不清除探针或日志。

## 2026-10-26 后续证据

[第二批现场与匹配 dump 分析](issue-1266-feedback-6416406209.md) 已把旧 Run2 的消失定位为一次 `0xc000001d` 异常，不能继续称为已证明的非崩溃退出。该转储不是两次新堆崩溃的 dump；新报告选择 A1 所有权实验，不改写本报告记录的上一轮 A0 判断。
