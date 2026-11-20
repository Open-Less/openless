# Issue #1264 Windows 语音快捷键联动调研报告

> 调研日期：2026-10-25
>
> 范围：提交 `38fbe1c082e782e566e13769e4675a183ccc812a` 的 Windows/桌面端源码；调研时工作树与 upstream/beta 一致。仅静态调研，不实施功能，不运行构建、测试或打包。

## 需求与关联背景

[Issue #1264](https://github.com/Open-Less/openless/issues/1264) 要求：总开关开启后，新增“与语音快捷键联动”子选项；第一次点击录音快捷键开启持续本地唤醒监听，第二次点击临时关闭并释放麦克风，而不是反复进入设置修改总开关。维护者已[回复“oka”](https://github.com/Open-Less/openless/issues/1264#issuecomment-6387227197)，尚未补充启动、模式冲突或录音中切换的语义。调研时 Issue 仍 OPEN，未发现关联实现 PR。

- [#1240](https://github.com/Open-Less/openless/pull/1240) 于 2026-10-21 合并，为本需求提供 Windows 本地唤醒基础；其描述仍列出完成词残留、Windows `0xC0000374` 堆损坏来源未定位、超时按总录音时长等未验收问题。这些是继承风险，不属于本 Issue 应顺手扩大的修复范围。
- [#1263](https://github.com/Open-Less/openless/pull/1263) 当前 OPEN，只补 ScrollLock 快捷键支持，明确不含语音唤醒开关联动；不是本需求的替代实现，也不是使用其他已支持快捷键的前置条件。
- [#1235](https://github.com/Open-Less/openless/pull/1235) 当前 OPEN，增加 Toggle 模式 Enter 停止录音，会触及 Core 偏好、coordinator/hotkey_loops 和录音设置页，后续集成需组合回归。

## 1. 结论摘要

- **[已证实，现状]** 语音唤醒已有 Core 生命周期和 Windows runtime，但没有“与录音快捷键联动”的持久字段，也没有把普通录音快捷键改成“首次开启监听、再次关闭监听”的策略（`ports.rs:117-142`、`api.rs:3415-3432`）。[1]
- **[已证实，隐私]** 唤醒前固定使用本地 Sherpa，standby 音频不发送云端；唤醒后的正式正文走独立 ASR provider，可能是云端，不能把“本地唤醒”表述成“全流程本地”（`shared_types.rs:462-476`、`core_adapters.rs:1942-1957`、`api.rs:6364-6397`）。[2]
- **[已证实，风险]** `VoiceActivationHandle::stop()` 会取消 recognizer、清理 activation consumer，但随后可能按 low-latency 偏好保持 warm stream，不能保证 Issue 所说的“释放麦克风”（`recorder.rs:115-122`、`core_adapters.rs:2236-2274`）。[3]
- **[推断，方案]** 可复用现有 handle、generation、事件和 `WarmCaptureHub`，但必须增加 Core-owned linked policy、临时暂停状态和 effective warm-capture 约束。

## 2. 现状与本地/云端边界

`UserPreferences` 持久化总开关、唤醒/结束词、超时和设备名，设置变化会触发 runtime 重配（`commands/settings.rs:242-250,309-321`）；注释明确写明 standby audio never sent to a cloud provider（`shared_types.rs:462-476`）。[5] Windows runtime 解析固定 `SherpaOnnx` 与 `zipformer-bilingual-zh-en-streaming`，模型未安装就返回 ModelMissing；唤醒 recognizer 通过 `WarmCaptureHub` arm（`core_adapters.rs:1942-1957,2039-2048`）。[6] activation VAD 使用 RMS `0.015`、约 400 ms pre-roll 和 8 个 hangover chunks，静音块不直接送 recognizer（`recorder.rs:49-55,447-487`）。[7][17] 上述本地边界只适用于唤醒前音频，不代表唤醒后的正式听写和后续处理都离线；本轮未做运行时网络抓取验证。

唤醒信号经 generation 校验后，Core 调用 `handoff_to_dictation`；Windows 取消 wake ASR，并可创建本地 finish recognizer（`api.rs:3692-3758`、`core_adapters.rs:2115-2173`）。[8][18] 随后正式 dictation 通过 `resolve_dictation_asr_provider` 按当前偏好选 provider；共享云端 provider 仅在特定未配置条件下切到本地 fallback（`api.rs:6364-6397,9253-9264`）。[9][19] 因此“正文必须本地”是**待定义**，不能由现有语音唤醒开关推导。

启动阶段 Core 会先使旧 generation 失效，再停止旧 handle；只有偏好启用、host runtime 可用且 capture 能成功 arm 时才进入等待唤醒，旧回调会因 generation 不匹配被丢弃（`openless-all/app/crates/openless-core/src/api.rs:3415-3432,3692-3713`）。这说明 linked mode 应复用现有异步生命周期，而不是在 Tauri coordinator 另建一套状态机。设置保存会校验短语和超时，并在相关字段变化后触发 runtime 重配；临时快捷键状态则不能走持久化设置路径。
## 3. Core 接缝、临时状态与设备释放

最合适的接缝是 `VoiceActivationRuntime::configure`、`VoiceActivationHandle::stop`/`handoff_to_dictation`、generation 和 `VoiceActivationStateChanged`。EventBus 已按 generation 保存最新状态，Tauri 已转发 `voice-activation:state-changed`（`ports.rs:117-142`、`events.rs:410-515`、`tauri_events.rs:213-215`）。[4][10][20]

建议新增类似 `voiceActivationHotkeyLinked` 的**持久偏好**，但“当前因快捷键开启/暂停”只能是运行时状态，不能写回设置。首次完成按键仲裁的有效点击只 arm listener，第二次 press 进入临时暂停；普通 dictation 的 FIFO、generation 和 stale-event 防护继续由 Core 处理。Issue 没有规定只能 Toggle：Toggle 可以是首个实现选项，也可以另行定义 Auto 短按 toggle、Hold 按住监听，需先确认语义。

必须正视释放坑：`warm_capture_enabled` 的公式是 `voice_activation || (low_latency && !low_memory)`（`recorder.rs:115-122`）。`handle.stop()` 清 consumer、取消 wake/finish ASR 后，又以 `voice_activation=false` 按普通偏好配置 warm hub；low-latency 开启时仍可能预热麦克风（`core_adapters.rs:2236-2274`）。所以 stop 可复用，但不是硬释放保证；临时 pause 必须同时关闭 effective warm policy，并在需要时停止 native stream。

还要抑制自动 rearm：正式录音释放后存在 `configure_voice_activation()` 重配（`core_adapters.rs:3525-3535`）；结束词完成和错误恢复也会在持久 `voice_activation_enabled` 为真时重新 arm（`api.rs:3764-3774,3842-3850,3884-3890,4022-4055`）。[11][13][14][15] 状态机必须区分“持久启用”和“当前暂停”，所有 reconfigure/recovery 都检查后者。
普通 dictation 的 provisional capture 还会在 warm 与 cold 两条路径之间切换：cold capture 会暂停 warm stream，录音结束再恢复；voice activation handoff 则使用带 pre-roll 的 staged warm capture。因而 Core 的 voice session gate 与 Tauri 的 WarmCaptureHub 是两层资源协调，不能只用一个 Busy 标志解释所有麦克风竞争。

## 4. 需定义语义、验证重点
建议最小运行状态包括 LinkedDisabled、LinkedStarting、LinkedListeningForWake、LinkedStopping、LinkedPaused 和 LinkedUnavailable/Failed；它们与持久 `voice_activation_enabled` 分离。首次点击失败时应保留明确错误而不回退成普通 dictation，第二次点击若处于 handoff 或停止中应按 generation 防止重复 stop；重新启用必须由下一次明确点击或产品定义的恢复动作触发。

**[待定义]** 当前 Auto 用 350 ms 区分短按/长按，并有 debounce、cooldown、modifier grace（`hotkey_interpreter.rs:7-19`）。[16] 需明确四种模式的 linked 行为，以及 listener 在 Starting、Handoff、结束词收尾，或 Dictation、LessComputer、SelectionVoice、QA、TTS 活跃时应取消、排队、Busy 还是忽略。本次源码未证实 TTS 的麦克风抑制策略。`VoiceSessionGate` 只明确互斥 Dictation、LessComputer、SelectionVoice、QA（`voice_session.rs:5-79`）；standby listener 与 TTS 的关系仍待验证。[12]

前端已有完整 phase 类型和状态事件通道，但设置页当前主要用事件判断模型缺失；linked mode 还需要把“已暂停但持久开启”与 Disabled、MicBusy、PermissionDenied 分开显示，避免用户误以为持久配置被清除。
**[测试重点]** 现有单测已覆盖短语规范化、revision 去重、generation/session 防陈旧和 warm policy（`voice_activation.rs:428-565`、`recorder.rs:3244-3255`）。[22]新增用例应覆盖首次/二次点击、重复点击/stale release、暂停不持久化且不自动 rearm、low-latency 下真实 stream 释放、cold/warm 交替、设备不存在、权限拒绝、其他程序占用、模型缺失、唤醒后云端/本地 provider、结束词和超时恢复。资源释放与权限必须在 Windows 实机验证，静态单测不能替代。
实机还应观察系统麦克风指示灯、设备切换后的实际占用、录音结束后的恢复时序，以及唤醒前 PCM 是否只到本地 recognizer；云端正文则应通过 provider 日志或网络策略单独验证，不能把状态事件当成隐私证明。

## 5. 推荐交互与改动边界

**推荐方案而非已实现行为：** 联动偏好默认关闭，保证旧用户行为不变；启用联动后，快捷键操作运行时监听状态，不反复写总开关。建议每次应用启动后先保持临时暂停，等待首次显式点击；是否恢复上次监听状态仍需产品确认。联动开启期间是否统一采用单击切换、覆盖原 Hold/Auto/DoubleClick 语义，应明确说明，不能暗中只支持 Toggle 而缩减需求。联动关闭时恢复原录音模式。

设置 UI 复用现有 Windows 语音唤醒内联子区域（`RecordingInputSection.tsx:617-665`），新增开关与常驻说明，不需要 Modal/Drawer/二次确认。[23] 显示“唤醒监听中 / 已临时暂停 / 正在切换 / 无法启动”等真实状态；Toast 只辅助反馈，权限、模型缺失等错误保留可操作的恢复入口。文案应写清“等待唤醒时仅本地处理；开始正式听写后按当前服务配置处理”。异步完成后才显示成功，不夺输入焦点；失败不悄悄回退为普通录音。此处为 UI 选型建议，尚未运行 UI 验证。

最小改动面不只是一个 TSX 开关：Core 偏好及 serde/wire 往返、前端类型/默认值和文案、Core hotkey dispatch 与监听有效状态、Tauri warm-capture 策略及设置重配入口、必要的偏好同步边界和回归测试。建议让 Core 统一拥有联动策略，复用现有 Windows runtime，暂不扩展到其他平台。持久联动偏好是否跨设备同步可沿用项目偏好分类；临时暂停态不能同步。

## 6. 远程验证要求

本轮只有文档变更，不触发构建。后续实施并推送代码时，按准确工作分支和提交 SHA 执行 GitHub Actions 的 `ci.yml`、`release-tauri.yml`（桌面全平台）和 `android-apk.yml`（至少 aarch64）；优先交付已就绪的桌面产物，不把其他 SHA 的成功结果移用到新代码。[24][25][26]

现有 CI 的 Windows 完整 Tauri 后端测试使用 `--no-run`，只编译；实际执行的是独立 `backend-tests` 和 recorder 模块测试。完整 Core 测试另在 Linux 检查中执行。[27][28] 因此应把新增联动状态机尽量放到可独立执行的 Rust-only 用例中，再用 Windows 实机验收真实设备释放、快速切换和权限恢复；不能把编译成功、安装包生成或状态事件变化当成硬件生命周期通过。

额外回归必须包括：暂停后保存设置不会重新预热（`commands/settings.rs:221-236`）、low-latency 开启时仍能释放唤醒所持资源、旧恢复回调不会重新 arm；其他语音任务仍在使用麦克风时要说明占用来源，不能谎报整个应用已释放设备，也不能静默丢弃进行中的正文。[29]

## 7. 证据边界

本报告完成的是源码与 GitHub 一手材料调研，只新增此 Markdown；未修改功能代码，未运行本地编译、测试、打包或依赖构建，未提交、推送或发布 Issue 评论。所有推荐交互和新增测试均待实施与验证，不把既有 PR 的未验收事项说成已经解决。

[1]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/ports.rs#L117-L142
[2]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/core_adapters.rs#L1942-L1957
[3]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/core_adapters.rs#L2236-L2274
[4]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/events.rs#L410-L515
[5]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/shared_types.rs#L462-L476
[6]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/core_adapters.rs#L1942-L1957
[7]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/recorder.rs#L49-L55
[8]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L3692-L3758
[9]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L6364-L6397
[10]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/events.rs#L410-L515
[11]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L3764-L3774
[12]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/voice_session.rs#L5-L79
[13]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/core_adapters.rs#L3525-L3535
[14]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L4022-L4055
[15]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L3842-L3890
[16]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/hotkey_interpreter.rs#L7-L19
[17]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/recorder.rs#L447-L487
[18]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/core_adapters.rs#L2115-L2173
[19]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L9253-L9264
[20]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/tauri_events.rs#L213-L215
[21]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/recorder.rs#L115-L122
[22]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/voice_activation.rs#L428-L565

[23]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src/pages/settings/RecordingInputSection.tsx#L617-L665
[24]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/.github/workflows/ci.yml#L10-L21
[25]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/.github/workflows/release-tauri.yml#L31-L42
[26]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/.github/workflows/android-apk.yml#L27-L41
[27]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/.github/workflows/ci.yml#L723-L750
[28]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/.github/workflows/check-linux-egui.yml#L108-L119
[29]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/commands/settings.rs#L221-L236
