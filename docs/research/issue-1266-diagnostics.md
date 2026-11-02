# #1266 Windows 无响应诊断包使用与证据边界

当前为 **A1 单变量修复实验**，不是已确认的根因修复。第二批现场、精确 dump 符号与资格判定见
[本轮分析](issue-1266-feedback-6416406209.md)。只改变 Windows event-loop target 的所有权，
不将它等同于已解决所有堆崩溃。上游基线为
`38fbe1c082e782e566e13769e4675a183ccc812a`；原始现象、N1–N7 合约、
H1–H5 假设及原始版本符号入口见 [研究报告](issue-1266.md)。
#1227 新反馈与 PR #1267 的对照、语义分类及当前修复阻塞见
[现场分析](issue-1227-feedback-pr-1267.md)；不能将这些问题直接视为同一根因。
除上述所有权变量，本次不调整按键 FIFO、取消保护、IME 超时/未知结果不重放、
设备/ASR、重试或录音清理顺序。没有本地编译、测试、依赖安装或打包。

## 构建及校验

- Cargo feature `issue-1266-diagnostics` 默认关闭；只有 Windows 启用真实 writer。
- `Release Tauri (cross-platform)` 的手动分支构建选择
  `windows_hang_diagnostics=true`；不得填 `release_tag`，不发布 release。
  `platform=all` 时 macOS 仍是普通构建，Android 不启用该 feature。
- Windows 诊断 artifact 名为 `openless-windows-x64-issue-1266-diagnostics`。
  普通分支源码 checkout 固定为该 run 的 `github.sha`。
- 包内 `openless-provenance.json` 明示准确 source SHA 和 feature 状态；
  `openless-symbol-pair.json` 验证原生 EXE 的 CodeView GUID+Age 与 PDB 相等，
  记录 EXE/PDB SHA-256、source revision 和 run URL。校验失败不上传 Windows 包。
  安装程序与安装后的原生 EXE 不是同一个文件，不能混用哈希。
- CI 运行现有默认门禁；另在 Windows/macOS 实际执行引用生产模块的独立
  sink 测试（off/on），注入内存/故障 writer，不打开现场日志。
  两平台另编译 feature-on 的生产宿主；Windows 完整 Tauri `--no-run` 仅算编译覆盖。
  单独的非测试 façade 可执行文件验证关闭 feature/非 Windows 时不初始化 writer。
- 静态契约和队列/格式测试不是原始无响应的重现，更不能证明根因已修复。

## 唯一会话文件

```text
%TEMP%\openless-issue-1266-2e2c7a47-session.ndjson
```

该文件只追加，不覆盖、不截断、不轮转。当前开发过程不运行应用，也不创建此文件。
每次进程启动生成不同的 run-local 标签；不要把相邻记录当作同一录音或因果链。
后续单变量修复仍使用此文件，以新的 attempt/change 身份区分；当前为
`A1-owner-thread-target` / `C1-windows-target-storage`；旧 A0 记录继续只读保留。

新 producer 只生成固定尺寸的枚举/计数记录，使用容量 512 的 `try_send`。
序列化和磁盘 I/O 只发生在一个 writer；启动时预初始化，不在首个 probe 中启动线程。
没有 heartbeat、定时唤醒、每 PCM/IME query 日志或新增重试。
标准库队列并非保证 lock-free；时间读取、原子计数、闭包包装、队列及 I/O 都可能
扰动时序。如果原症状在诊断版本中不能重现，应判 INCONCLUSIVE，而不是有效修复。

新 payload 不含录音/识别文本、提示词、凭据、原始错误、URL、文件路径、窗口/设备名、
HWND、PID 或 TID。`operation_id`/`parent_operation_id` 是本次运行生成的数字标签；
`thread_label` 是 producer 线程的运行内 opaque 数字，不是 OS TID/PID 或线程名；
不同 run 的同号没有关联。`event_seq` 是每 probe 的源序列，不是全局时钟。`monotonic_us` 为入队边界的单调耗时，
`ts` 是初始化墙钟加该耗时的毫秒时间戳。并发记录的文件顺序不保证等于采样顺序。
`safe.kind` 是闭合类别，`safe.value` 仅用于内部终态枚举或 query_count 事件的查询次数，`safe.flag` 用于 Hold 模式。

旧 voice-1003 的任意 JSON 不转发；仅 `cancel entered/returned` 计入 `legacy_mapped`，
其余计 `legacy_unmapped`。这些是兼容性覆盖计数，不构成六个 probe 的证据，
不保留旧诊断的详细事实；feature 开启时绝不 fallback 到旧文件 writer。
旧普通应用日志及其中既有诊断仍保持原样，可能含敏感内容，不应公开上传。

## 六个边界

| Probe | 假设 | 实际观察 | 明确不代表 |
|---|---|---|---|
| P1 | H1/H5 | 八类 supervisor 和设置/restore/style removal 共用 accessor 的 gate request/acquire/真实 drop 后 release | 单凭等待很久不能证明死锁；release 记录可被后来者的 acquire 超越 |
| P2 | H1/H5/H1227-1/H1266R2-1/2 | host 与 capsule 两种 adapter 的 handle clone-enter/return，以及到同一主线程队列的真实派发；kind 区分 host/capsule/mirror/layout；执行上下文提供真实 parent；保留 action 注册 ack | API 成功不等于执行；内联 entered 可先于 dispatch returned；parent 是派发来源，不单独证明同步重入；未改五秒预算 |
| P3 | H2 | dictation bridge 实际 consume；Pressed/Released 的模式、handler 真正开始和返回 | Hold spawn 返回不是 handler 完成；不证明未消费的物理 Released 已入队；combo/Esc 独立 bridge 不在覆盖内 |
| P4 | H3 | submit admission、blocking worker entry、找到窗口、真实 submit ack 和终态 | worker entry 可否定该请求的“未获调度”；不证明目标文档已显示正确文本 |
| P5 | H4 | stop admission/worker、cold recorder processor lock、join、archive send/join；真实嵌套 archive parent | join 慢不等于闭合资源等待；warm capture/reconfigure 内部细节不完整；Core 任务之间不编造 parent |
| P6 | H1–H5/H1227-4/H1266R2-1/2 | 原有 Ready/CloseRequested/Destroyed/ExitRequested/Exit callback，在原清理前观察 | 不是 WebView heartbeat；退出请求不是进程退出；缺少 Exit 不证明进程仍活着 |

P4 `returned` 的 `safe.value`：0=错误/未知，1=Committed，2=NotDispatched，
3=CompositionConflict，4=CompositionRecoveryRequired，5=Rejected，6=Failed。
query_count 只表示已尝试的查询数，不表示成功回复数。Committed 只代表后端终态；任何 `OutcomeUnknown` 都保持未知，不授权重放。
P2 的执行上下文只在真实闭包 entry 至离开期间存在，返回/unwind 恢复旧上下文；
排队 child 可能在 parent 离开后才进入，必须结合 thread_label/单调时间判断，不能仅凭 parent 说重入。
同一 P2 tracer 保留原 host 路径，并补上旧 #1261 异常实际发生的 capsule 路径；不是额外第七类探针。
高频胶囊派发会增加日志量，若观测扰动使症状消失，仍判 INCONCLUSIVE。

`handle_clone_returned` 只说明 handle 克隆/获取调用返回，可能仍为 unbound；不代表派发或 entry。
P2/P5 的 `returned` 表示该层调用返回，不是整轮成功；`scope_left` 包含取消/展开，
绝不是成功。P4 外部 future 与 blocking worker 的 scope 可能分别离开，仍须看明确终态。

## 独立健康状态

仅诊断 feature 的 Windows 包注册只读 `issue1266_health` IPC，无额外前端/浮层。
在可用的 Tauri 开发者控制台中，可调用：

```javascript
await window.__TAURI_INTERNALS__.invoke('issue1266_health')
```

返回 build/run 身份、initialized/ready、dropped/queue_closed、legacy 计数、
spawn_failed/io_failed 和 written/flushed。若安装包不提供控制台，不要求用户修改
系统或安装包以打开它；拿不到 health 就明确标注 coverage unknown，并保留外部现场。
该命令不经文件队列自报，但仍依赖可响应的 IPC，不是挂死时必定可用的备用通路。
快照非事务；flush 是 OS 写入水位，不保证断电持久化。文件里历史零丢失不能排除之后
队列满、writer 失效或最后一行写入中断；缺少事件不能单独认定对应代码未执行。

## 复现与安全交付

**请你用修复版复现 bug**

1. 从对应准确 SHA 的 Actions 下载诊断 Windows artifact，保存其中的 EXE、PDB 和两份 manifest；安装诊断包，不更改原来的麦克风、ASR、输入模式和快捷键。
2. 保持原场景；空闲观察至少覆盖此前约 7 小时 24 分钟，活动场景使用可比较负载，不以短时未崩判修复。按原使用方式触发录音、松键、识别及插入；记录实际可见结果（正确/错误/无变化/卡住/重复/未知），不要只反馈日志是否有 ERROR。
3. 若卡住，先记录发生时间和该进程是否还存在，分别记录主窗口、胶囊、托盘和热键是否响应；不要立即结束进程或切换配置掩盖现场。
4. 若具备受控取证条件，用原 PID 私下保留两份间隔约十秒的 full dump（例如 ProcDump `procdump -ma PID` 两次）。此间隔只是采样，不是产品延迟；相同栈本身也不证明死锁。必要时另保留相关 WebView2 子进程现场。
5. 通过私密渠道返回唯一会话文件、可获得的 health 和可见行为观察。dump、普通应用日志和录音可能包含完整内存/文本/凭据，不要公开上传。用户确认原始症状消失前保留探针与会话文件。
