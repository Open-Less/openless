# 06：事件、会话与取消

入口：[Core events.rs](../../openless-all/app/crates/openless-core/src/events.rs)、[types.rs](../../openless-all/app/crates/openless-core/src/types.rs)、[Linux主循环](../../openless-all/app/linux-egui/src/main.rs)。
当前Linux已有事件消费/重放和若干时序修复，正式UI应保留这些语义，不仅复制页面布局。

## 1. 订阅与恢复

1. 建立订阅后初始化Core，读取startup和领域快照，检查`running`及`contract_version`。
2. 按`sequence`去重，按`session_id`归属分发；不能将任何最新事件都归给当前页面。
3. 消费落后时使用已有replay/快照恢复机制；不要跳过缺失事件后继续在旧状态上追加。
4. 配置变更按revision刷新缓存；UI草稿不能反向覆盖较新的后台配置。
5. 事件到达后请求重绘；不要在每帧启动新订阅、网络请求或后台任务。

恢复时保留各领域的不同策略：听写恢复状态，QA以完整messages校准，Less Computer按轮归属处理；不是一个通用“取最新事件”就能替代。

听写`DictationStateSnapshot.recording_ready`的JSON字段为`recordingReady`，每轮初始为`false`；`phase=recording`仅说明原生启动已返回，不能据此提前显示麦克风就绪。AudioRecorder必须先向consumer交付非空PCM，再报告该帧的level；Core仅在这个首帧回调后将`recordingReady`置为`true`，即使首帧是`elapsedMs=0,level=0`也必须发布状态。不得用启动定时器或预填零电平伪造首帧。UI在`starting/recording`且`recordingReady=false`时显示待命；终态退出待命，新会话重新从`false`开始。旧JSON缺字段时Core按`false`读取。

Less Computer语音提供`voice_state {sessionId, phase, level, elapsedMs}`及`BackendEventPublisher::latest_less_computer_voice_state()`固定一条最新投影。`phase`为`starting/recording/transcribing/idle`；投影保留原seq，供阶段事件被有界replay驱逐后恢复显示，不能推进聊天去重水位。Linux Host/UI接手此显示消费；旧session的Idle或迟到电平不得覆盖当前录音。
该语音投影复用相同首PCM合同：原生start返回后仍保留`starting`，首帧level才转为`recording`；Host/UI不能自行补发就绪态。

## 2. TranscriptDelta不是简单追加

`offset`表示Unicode scalar（Rust `char`）数量，不是UTF-8字节或UTF-16单元。
新文本为旧文本前`offset`个scalar加`delta.text`，原尾段被替换。优先复用`TranscriptAccumulator`。

例：已有`你好世`，收到`offset=2,text="世界"`，结果是`你好世界`，不是`你好世世界`。
中文、emoji、provider interim回修与最终delta都需测试。越界offset是错误，不截断猜测；迟到的上一轮delta不能改新轮文本。

## 3. QA与Less Computer的归属

- QA对话owner与每轮录音token不同。停止录音使用该轮token，不能把延迟的静音回调转换为无条件toggle。
- QA增量事件的`messages=None`并不表示清空历史；追加chunk，完整消息到达时再校准。
- 面板关闭/重开时丢弃旧owner的迟到输出；语音finish期间仍须能取消实际ASR。
- Less Computer每个新User轮更新当前session，`fresh`仅决定是否清除会话显示；续聊也必须接收新轮输出与审批。
- 工具/审批事件按Core标识关联，不在UI另建可绕过Core的approval token registry。
- Less Computer录音反馈使用typed `voice_state`：`sessionId`、`phase`（starting/recording/transcribing/idle）、`level`、`elapsedMs`。与同一事件流的sequence一起消费，旧session的idle/电平不能清除新录音；Linux页面接入这组反馈仍由egui团队完成。

源码参考：[qa.rs](../../openless-all/app/linux-egui/src/qa.rs)、[Core QA/Agent接口](../../openless-all/app/crates/openless-core/src/domains.rs)。

## 4. 取消和原生资源

共享语音使用Busy拒绝新会话。停止录音、取消整个会话和正常结束不是同一动作。
异步启动前登记owner；每个慢await返回后核验身份和取消状态，迟到得到的资源直接关闭。

录音移交给finish后仍要保留共享取消句柄；不能`take()`唯一资源后让取消路径找不到ASR。
Core已有capture控制与单一终态规则；Host保留有效句柄并如实返回错误，UI只显示最终状态。
Core资源hold覆盖在途原生初始化与收尾；关闭页面、取消回复或丢弃调用future不能让旧任务恢复新会话的静音/输入源。不要把Core暂时返回Busy当成可以在Host绕开的锁。

Remote socket下行只发本连接所属session；客户端stop后到finish结束之间仍保留取消路由。
停止native listener/服务后再shutdown Core；退出过程中不接新业务。

## 5. 插入结果

- `Inserted`：原生确认写入；`PasteSent`：派发粘贴但不能证明目标接收。
- `CopiedFallback`：已经复制且需要用户处理；`NotRequested`：本次没有请求插入。
- `OutcomeUnknown`是错误语义：原生可能已写入，不能自动重试造成双写。

UI通知、历史和Remote结果不得把这些统一显示为“输入成功”。

## 6. 最小回归集合

重复/乱序/lag重放、Unicode回修、两轮QA、两轮Less Computer、旧session迟到事件；Starting/录音/转写/finish各阶段取消；旧静音回调到达新轮；面板关闭重开；插入未知结果不重复提交。
复用[host contract](../../openless-all/app/linux-egui/tests/host_contract.rs)和Core各领域contract；fixture通过之后仍需验证原生Host效果。
