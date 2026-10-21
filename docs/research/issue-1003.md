# Issue #1003 语音激活输入法研究报告

> 调研日期：2026-10-18
>
> 研究范围：GitHub Issue #1003、PR #1219、PR 关联提交，以及 `beta` 与 `feat/issue-1003-voice-activation` 的源码。本文只记录可追溯的主源证据；没有把 PR 的声明当作本次本地运行结果。

## 1. 结论摘要

| 问题 | 结论 | 证据 |
| --- | --- | --- |
| 功能是否已经进入当前 `beta` | **没有。** Issue 仍 OPEN；PR #1219 仍 OPEN、Draft、`REVIEW_REQUIRED`、`BLOCKED`，没有 merge commit。当前 `beta` 是 `de072a88`，而 PR head 是 fork `HKLHaoBin/openless` 的 `9270b43c`。 | [Issue #1003](https://github.com/Open-Less/openless/issues/1003)、[PR #1219](https://github.com/Open-Less/openless/pull/1219) |
| 分支落后情况 | 以当前 `beta` `de072a88373a222280a8c22948092a260880196c` 和功能分支 `9270b43c7e79ad9d462ad8bec78250de5f499093` 比较：功能分支 **领先 8、落后 3**，两边分叉，合并基点为 `96278f40d48b8ed02e3ce22ddc96b694c863a6bc`。落后的 3 个提交是当前 beta 新增的 `af8adf7e`、`5ab6ed43`、`de072a88`。 | [PR base/head 元数据](https://github.com/Open-Less/openless/pull/1219)、[功能分支提交](https://github.com/HKLHaoBin/openless/commits/feat/issue-1003-voice-activation)；本地 `git rev-list beta..feat/issue-1003-voice-activation` 与反向计数为 8/3 |
| 重启后持久化 `voiceActivationEnabled` 是否自启动 | **条件性地是。** `voice_activation_enabled` 以偏好字段持久化；应用第一次调用 `get_startup_snapshot` 时，Core `start()` 读取当前偏好并调用 `configure_voice_activation()`。模型已安装、权限可用且麦克风未占用时会进入等待唤醒；模型缺失、权限拒绝或麦克风忙时只会报告失败状态。 | [`config.rs:395-465`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/config.rs#L395-L465)、[`App.tsx:84-96`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/App.tsx#L84-L96)、[`shared.ts:45-57`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/lib/ipc/shared.ts#L45-L57)、[`api.rs:4530-4609`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L4530-L4609) |
| 连续监听是否有 VAD/CPU gate | **没有找到。** 空闲 warm capture 的每个 PCM chunk 都会直接交给 activation consumer；Sherpa online worker 对每个收到的 PCM 调用 `accept_pcm_chunk`。现有环形缓冲和录音队列是容量/线程安全机制，不是静音跳过或 CPU 门控。 | [`recorder.rs:594-645`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/recorder.rs#L594-L645)、[`sherpa_provider.rs:244-255`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L244-L255)、[`sherpa_provider.rs:292-343`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L292-L343) |
| 结束词带标点是否清理 | **已在功能分支实现。** NFKC、空白和终端/中英文标点会参与归一化；匹配仅接受文本后缀；从原始 transcript 尾部逆向剥离结束词时跳过标点和空白，保留正文本身的标点。测试覆盖 `完成，语音录入。`、全角句号、正文标点和中间出现结束词的反例。 | [`voice_activation.rs:49-115`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs#L49-L115)、[`voice_activation.rs:390-445`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs#L390-L445)、[`api.rs:2264-2321`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L2264-L2321) |

**一句话判断：** PR 已完成“本地 Windows 语音唤醒/结束词”的主体链路以及结束词标点修复，但尚未合并到当前 beta；持续监听仍把静音 PCM 全部送入 Zipformer，CPU/VAD 问题是合并前必须明确验收或拆分处理的风险。

## 2. 主源与需求演进

### 2.1 原始需求

Issue 标题为“功能建议：用语音替代按键来激活输入法”。原始描述提出：说“开始语音录入”，录入正文，再说“完成语音录入”（或等待超时）；使用较书面的词以降低日常口语误激活，并希望命令可自定义、识别尽可能本地化。[Issue #1003](https://github.com/Open-Less/openless/issues/1003)

### 2.2 讨论中新增的约束

1. **按键替代场景。** 评论说明右 Ctrl/快捷键会干扰肌肉记忆，也有人用 ScrLock 或鼠标侧键折衷。[评论 5436896181](https://github.com/Open-Less/openless/issues/1003#issuecomment-5436896181)、[评论 5441322107](https://github.com/Open-Less/openless/issues/1003#issuecomment-5441322107)
2. **常驻监听的系统成本。** 评论明确提出全天开麦可能耗电、持续占用 CPU，并可能占用 Windows 麦克风导致其他应用难以使用。[评论 5596617615](https://github.com/Open-Less/openless/issues/1003#issuecomment-5596617615)
3. **本地模型引导。** 维护者在 2026-10-14 发布模型缺失引导及 Windows 测试步骤，要求模型未安装时给出跳转按钮，安装后重新配置/回到设置确认进入等待唤醒。[评论 6170844277](https://github.com/Open-Less/openless/issues/1003#issuecomment-6170844277)
4. **结束词的 ASR 断句。** 用户报告“完成，语音录入。”会被混入正文；随后 PR 提交 `37e4665d` 宣称修复该问题并增加回归测试。[评论 6216314465](https://github.com/Open-Less/openless/issues/1003#issuecomment-6216314465)、[提交 37e4665d](https://github.com/HKLHaoBin/openless/commit/37e4665d78a39011431401393264002a4879c197)
5. **VAD/CPU 争议。** 用户建议无人在说话时不要把音频交给 Zipformer；维护者先认为额外人声模型可能增加 CPU，之后在讨论中接受这是应解决的优化，并表示可另行拆分以保持 PR 可维护。[评论 6218184776](https://github.com/Open-Less/openless/issues/1003#issuecomment-6218184776)、[评论 6219721968](https://github.com/Open-Less/openless/issues/1003#issuecomment-6219721968)、[评论 6221936954](https://github.com/Open-Less/openless/issues/1003#issuecomment-6221936954)、[评论 6231986920](https://github.com/Open-Less/openless/issues/1003#issuecomment-6231986920)、[评论 6236232646](https://github.com/Open-Less/openless/issues/1003#issuecomment-6236232646)

### 2.3 PR 及修复提交序列

- `b0196a9e`：加入可配置 voice activation 的主体实现。[提交 b0196a9e](https://github.com/HKLHaoBin/openless/commit/b0196a9ed9bb1358c4d4bb1fb9b7cbf8ca4c66b3)
- `65afa352`：补充模型缺失引导。[提交 65afa352](https://github.com/HKLHaoBin/openless/commit/65afa3521be38cf1c143f3ddff1635fc18fa99d3)
- `37e4665d`：容忍结束词中间插入标点，并清理 transcript。[提交 37e4665d](https://github.com/HKLHaoBin/openless/commit/37e4665d78a39011431401393264002a4879c197)
- `d66c07c4`、`3e56d3a3`：功能分支合入当时的 beta；提交信息显示合并时曾处理多处冲突。[提交 d66c07c4](https://github.com/HKLHaoBin/openless/commit/d66c07c4f4a60d6fb5af4acf14aef3c94b2676cd)、[提交 3e56d3a3](https://github.com/HKLHaoBin/openless/commit/3e56d3a3c214a474cff10df34810c18660c6bf46)
- `9270b43c`：更新 warm capture 测试以适配 preroll 标志，当前 PR head。[提交 9270b43c](https://github.com/HKLHaoBin/openless/commit/9270b43c7e79ad9d462ad8bec78250de5f499093)

PR #1219 的 body 写明 `Closes #1003`，并列出 Core 测试、格式检查、Bugbot 复审和三个 Actions 运行，但 PR 状态本身仍是 Draft/待 Review。[PR #1219](https://github.com/Open-Less/openless/pull/1219)

## 3. 当前 beta 与 PR 状态

### 3.1 合并状态

截至调研日期，GitHub PR 元数据为：

- base：`Open-Less/openless:beta`，PR 记录的 base SHA 为 `96278f40`；
- head：`HKLHaoBin/openless:feat/issue-1003-voice-activation`，head SHA 为 `9270b43c`；
- `state: OPEN`、`isDraft: true`、`reviewDecision: REVIEW_REQUIRED`、`mergeStateStatus: BLOCKED`；
- `mergeCommit: null`、`mergedAt: null`。

这些字段来自 [PR #1219](https://github.com/Open-Less/openless/pull/1219)。因此，功能分支曾把 beta 合入自身，**不能反推 beta 已经合入功能分支**；当前 beta 仍未包含该 PR。当前 beta 的本地 ref 为 `de072a88`，且功能新增文件 `openless-all/app/crates/openless-core/src/voice_activation.rs` 在当前 beta 中不存在。

### 3.2 落后/领先关系

以本地已存在的两个 ref 做纯 Git 图比较：

```text
beta = de072a88373a222280a8c22948092a260880196c
feature = 9270b43c7e79ad9d462ad8bec78250de5f499093
feature ahead of beta = 8
feature behind beta = 3
merge-base = 96278f40d48b8ed02e3ce22ddc96b694c863a6bc
```

功能分支落后的三个提交分别是：

- `af8adf7e fix(dictation): keep local startup stoppable and bound inference resources`；
- `5ab6ed43 feat(capsule): add persisted multi-display modes and prepare 2.0.2 Beta.1`；
- `de072a88 docs(release): verified 2.0.2 Beta.1 publication evidence [skip ci]`。

PR 的 base/head 和当前分支提交可在 [PR #1219](https://github.com/Open-Less/openless/pull/1219) 与 [功能分支提交列表](https://github.com/HKLHaoBin/openless/commits/feat/issue-1003-voice-activation) 复核。合并前应以当前 beta 重新合并/变基并重新解决冲突，不能直接使用 PR 创建时的 base 作为最新基线。

### 3.3 检查状态

PR 当前列出的检查均为通过，包括 Android cargo check、Linux Core/egui、Windows checks、macOS checks、macOS Rust 1.88 MSRV、UI motion 和 Changed areas。[PR checks](https://github.com/Open-Less/openless/pull/1219/checks)

这些是 CI/PR 的已有证据，不是本次调研重新运行的结果；本次没有修改业务代码、没有运行构建，也没有宣称已经在 Windows 实机复现。

## 4. 代码影响范围

PR 相对其 base 的 GitHub 统计为 **41 个文件、+3227/-201**。[PR #1219 changed files](https://github.com/Open-Less/openless/pull/1219/files)

| 层次 | 主要影响 | 关键源码证据 |
| --- | --- | --- |
| Core policy/state | 新增 phrase matcher、生命周期/代际、超时、状态信号、端口；`OpenLessBackend` 增加配置、唤醒 handoff、结束和恢复。 | [`voice_activation.rs`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs)、[`api.rs:3093-3285`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L3093-L3285)、[`ports.rs`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/ports.rs) |
| 偏好/持久化 | 增加启用、唤醒/结束开关、两条 phrase、超时和麦克风字段，并保留旧配置默认值。 | [`config.rs:395-465`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/config.rs#L395-L465)、[`config.rs:978-1028`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/config.rs#L978-L1028) |
| Windows host/音频 | Tauri 运行时加载 Zipformer、创建在线 Sherpa session、接入 warm capture、处理 mic/permission/model 错误和 generation。 | [`core_adapters.rs:1492-1851`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/core_adapters.rs#L1492-L1851)、[`recorder.rs:332-672`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/recorder.rs#L332-L672) |
| 本地 ASR | Sherpa provider/runtime 支持在线 chunk、token callback 和结束 flush。 | [`sherpa_provider.rs:78-255`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L78-L255)、[`sherpa_provider.rs:274-343`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L274-L343) |
| UI/IPC/本地化 | Recording Input 增加 Windows-only voice activation 设置、状态和模型缺失跳转；同步 mock/types/i18n。 | [`RecordingInputSection.tsx:146-200`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/pages/settings/RecordingInputSection.tsx#L146-L200)、[`RecordingInputSection.tsx:590-660`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/pages/settings/RecordingInputSection.tsx#L590-L660)、[`voiceActivation.ts:1-30`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/lib/voiceActivation.ts#L1-L30) |
| 测试/构建面 | Core phrase/lifecycle 测试、UI model-missing 测试、host contract、workflow 依赖变更。 | [`voice_activation.rs:390-503`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs#L390-L503)、[`voiceActivation.test.ts`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/lib/voiceActivation.test.ts) |

## 5. 已实现能力与行为链路

### 5.1 持久化与启动

`UserPreferences` 将 `voice_activation_enabled` 及相关字段纳入 camelCase wire contract，并给新增字段提供默认值。[`config.rs:395-465`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/config.rs#L395-L465)

启动链路是：

```text
App.tsx useEffect
  -> getStartupSnapshot()
  -> invoke('get_startup_snapshot')
  -> commands::get_startup_snapshot()
  -> core.start()
  -> get_preferences()
  -> configure_voice_activation()
  -> model check / Sherpa online listener / ListeningForWake
```

前端启动握手见 [`App.tsx:84-96`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/App.tsx#L84-L96) 与 [`shared.ts:45-57`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/lib/ipc/shared.ts#L45-L57)；命令和 Core 的调用见 [`commands/dictation.rs:3-8`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/commands/dictation.rs#L3-L8)、[`api.rs:4530-4609`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L4530-L4609)。因此，重启后的“自启动”不是偏好字段自身执行，而是应用首个 startup handshake 触发；若模型/权限/麦克风条件不满足，启动会发出失败状态而不会进入监听。

设置保存时，启用状态、phrase、超时和麦克风字段的变化会再次调用 `configure_voice_activation()`。[`commands/settings.rs:237-265`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/commands/settings.rs#L237-L265)、[`commands/settings.rs:284-317`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/commands/settings.rs#L284-L317)

### 5.2 唤醒、交接、超时和恢复

Windows runtime 检查固定的 `zipformer-bilingual-zh-en-streaming`，模型存在后创建在线 Sherpa ASR，并把 token callback 接到 `VoiceActivationEmitter`。[`core_adapters.rs:1701-1765`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/core_adapters.rs#L1701-L1765)

唤醒匹配后，Core 通过 generation 校验、warm-capture handoff 建立普通 dictation session，再保存 `VoiceActivationSession`；结束词或超时调用 `finish_voice_activation`，结束后按启用偏好重新配置监听。[`api.rs:3292-3528`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L3292-L3528)、[`api.rs:3565-3619`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L3565-L3619)。配置注释和停止旧 handle 的单麦克风约束见 [`api.rs:3093-3125`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L3093-L3125)。

### 5.3 结束词标点清理

实现分两层：

1. `normalize_phrase` 用 NFKC、去空白和标点处理 ASR phrase；`VoicePhraseMatcher` 只认文本后缀，避免正文中间出现 phrase 就误停。[`voice_activation.rs:49-60`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs#L49-L60)、[`voice_activation.rs:201-229`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs#L201-L229)
2. `strip_phrase_suffix` 从原始 transcript 尾部反向扫描，跳过空白/中英文终端标点，仅切掉结束词；Core 在每个 `TranscriptDelta` 累积 raw transcript 后，将清理后的文字写回 state 并发布实时事件。[`voice_activation.rs:62-115`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs#L62-L115)、[`api.rs:2264-2321`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L2264-L2321)

结束录音后，完成路径还会再次对 `engine_result.raw_text`、`engine_result.polished_text` 和可选 `engine_result.asr_transcript` 调用同一 suffix cleaner，再构造 `DictationResult`；对应实现见 [`api.rs:7291-7317`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L7291-L7317)。这些清理后的字段随后用于完成事件和历史持久化，历史写入字段见 [`api.rs:7470-7479`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L7470-L7479)、[`api.rs:7665-7752`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L7665-L7752)。该结论仍应通过 Windows 实机验证，因为不同 ASR 的 delta/revision 时序不等于单元测试输入。

### 5.4 模型缺失引导

runtime 在配置时检查模型是否安装，缺失返回 `ModelMissing`；UI 监听 `voice-activation:state-changed`，只对精确的模型缺失错误显示跳转模型页，避免把权限拒绝或麦克风占用误导成下载问题。[`core_adapters.rs:1718-1734`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/core_adapters.rs#L1718-L1734)、[`RecordingInputSection.tsx:146-200`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/pages/settings/RecordingInputSection.tsx#L146-L200)、[`voiceActivation.ts:21-30`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/lib/voiceActivation.ts#L21-L30)

## 6. 可复现缺口与风险

### 6.1 明确缺口：连续监听没有 VAD/CPU gate

代码路径是无条件转发：

- `WarmCaptureSink::consume_pcm_chunk` 先写 ring，然后在满足 activation 条件时直接 `activation.consume_pcm_chunk(pcm)`，没有能量阈值、VAD 状态或静音短路。[`recorder.rs:594-645`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/recorder.rs#L594-L645)
- `SherpaOnnxAsr::consume_pcm_chunk` 对 online worker 直接 `send_pcm(pcm)`；worker 收到每个 `Pcm` 后立即调用 `session.accept_pcm_chunk(&pcm, &emit)`。[`sherpa_provider.rs:244-255`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L244-L255)、[`sherpa_provider.rs:292-310`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L292-L310)
- `send_pcm` 使用标准无界 `mpsc::channel`，只检查空 chunk/取消，不检查音量或丢弃静音。[`sherpa_provider.rs:274-343`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L274-L343)

**复现方法：** Windows 安装并准备 Zipformer，开启语音激活，在安静房间保持 60 秒不说话；记录进程 CPU、Sherpa worker 消费量和 microphone 独占状态，再以相同条件说唤醒词。预期若有 gate，静音段应显著减少 inference；当前静态路径预期仍逐块送入模型。该实验尚未由本次调研运行。

### 6.2 模型下载后自动恢复未形成闭环的风险

当前明确调用 `configure_voice_activation()` 的位置是 startup、偏好保存、warm fallback 释放和运行时失败恢复；本地模型下载命令本身只调用 local-ASR service 的 `start_download`。[`api.rs:4530-4609`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L4530-L4609)、[`commands/settings.rs:284-317`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/commands/settings.rs#L284-L317)、[`commands/local_asr.rs:329-344`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/commands/local_asr.rs#L329-L344)。因此“下载完成后当前 listener 自动恢复”没有直接源码证据；Issue 的测试说明也要求返回录音设置或重新配置。[评论 6170844277](https://github.com/Open-Less/openless/issues/1003#issuecomment-6170844277)

这不一定是已确认 bug：下载流程可能由前端刷新或用户切换设置间接触发，需实机确认。若产品要求无刷新自动恢复，应添加下载完成事件到 voice activation reconfigure 的显式连接及回归测试。

### 6.3 麦克风占用、权限和模型缺失是启动条件，不是静默降级

配置 runtime 会检查模型；arm warm capture 失败时映射为权限拒绝、无输入设备、mic busy 或 runtime failure。[`core_adapters.rs:1498-1509`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/core_adapters.rs#L1498-L1509)、[`core_adapters.rs:1723-1795`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/core_adapters.rs#L1723-L1795)。应验证失败后热键录音仍可正常工作，且其他应用占用麦克风时 UI 不显示错误的“下载模型”建议；现有 UI 的错误分类测试只覆盖状态判别，未覆盖 Windows 设备竞争。[`voiceActivation.test.ts:12-42`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src/lib/voiceActivation.test.ts#L12-L42)

### 6.4 分支基线漂移与合并风险

功能分支已通过两个 merge commit 合入旧 beta，但当前 beta 又前进 3 个提交；且 PR 仍 blocked/review required。合并前必须用当前 beta 重新整合并复跑全部桌面、Android、Core/UI 检查，尤其关注 PR 曾经修改的 `api.rs`、`core_adapters.rs`、`recorder.rs`、settings UI 和 workflow 文件。[PR #1219](https://github.com/Open-Less/openless/pull/1219)、[PR changed files](https://github.com/Open-Less/openless/pull/1219/files)

## 7. 下一步实现建议

1. **先同步当前 beta，再决定 PR 边界。** 以 `de072a88` 为基线整合功能，解决冲突后重新跑 PR 全套检查；不要把“功能分支合入旧 beta”当作“beta 已合并”。
2. **为静音成本设定可测验收。** 在不改变唤醒召回率的前提下加入轻量、可解释的能量/频域 gate 或 sherpa 自带 VAD（若评估后成本可接受）；记录静音/有人声两类 CPU、延迟、漏唤醒和误唤醒指标。Issue 已有 VAD 建议和后续接受该优化方向的讨论。[评论 6218184776](https://github.com/Open-Less/openless/issues/1003#issuecomment-6218184776)、[评论 6236232646](https://github.com/Open-Less/openless/issues/1003#issuecomment-6236232646)
3. **避免无界积压。** 目前在线 PCM 使用无界 `mpsc::channel`；若推理速度低于采集速度，建议增加有界队列、明确丢帧/降采样策略和故障遥测，再用长时间静音及低性能 Windows 机器验证。[`sherpa_provider.rs:274-343`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs#L274-L343)
4. **闭合模型生命周期。** 模型下载成功后应触发一次明确的 voice activation reconfigure，或在 UI 中明确要求重新配置；覆盖“启用已保存 + 模型缺失 → 下载 → 无需重启即监听”的测试。
5. **补充真实设备验收。** 覆盖重启自动启动、权限拒绝、麦克风被其他应用占用、模型缺失/下载、热键与语音入口互斥、结束词断句、超时后自动恢复监听。
6. **把结束词清理保持在 raw/source 层验证。** 继续保留正文自身标点，测试普通逗号、全角标点、空格/换行、ASR revision、结束词出现在正文中间等组合，并核对实时 transcript、最终插入、history raw/asr/final 三套字段。[`voice_activation.rs:394-445`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/voice_activation.rs#L394-L445)、[`api.rs:2264-2321`](https://github.com/HKLHaoBin/openless/blob/9270b43c7e79ad9d462ad8bec78250de5f499093/openless-all/app/crates/openless-core/src/api.rs#L2264-L2321)

## 8. 验证清单

### 8.1 静态/CI

- [x] `gh issue view 1003`：Issue 状态、原始需求和评论已核对。[Issue #1003](https://github.com/Open-Less/openless/issues/1003)
- [x] `gh pr view 1219`：base/head、Draft、Review、merge 字段已核对。[PR #1219](https://github.com/Open-Less/openless/pull/1219)
- [x] Git 图：当前 beta `de072a88` 与 head `9270b43c` 的 ahead/behind 为 8/3。
- [x] PR 已有检查记录均为 pass。[PR checks](https://github.com/Open-Less/openless/pull/1219/checks)
- [x] 静态阅读确认启动、持久化、标点清理和每块 PCM 转发路径。
- [ ] 以当前 beta 重新整合功能分支并重跑 CI（本次调研未执行）。
- [ ] 运行 `cargo test -p openless-core`、voice activation 定向测试、`cargo fmt --all -- --check`（本次调研未执行；PR body 声称曾通过）。

### 8.2 Windows 实机

- [ ] 在设置 → 录音输入启用语音激活，确认安装模型后状态为等待唤醒。
- [ ] 关闭并重启应用，确认已持久化的 `voiceActivationEnabled` 在 startup handshake 后重新进入等待唤醒；分别测试模型存在、模型缺失、权限拒绝、麦克风忙。
- [ ] 说“开始语音录入”→正文→“完成语音录入”，确认唤醒词/结束词不进入正文。
- [ ] 重复测试“完成，语音录入。”、“完成语音录入。”、句末全角标点、词间空格/换行；确认正文原有标点保留。
- [ ] 语音激活开启后保持 60 秒静音，记录 CPU、内存、Sherpa inference 调用/队列量；与有人声的同长度样本比较，确认是否存在 VAD/CPU gate。
- [ ] 下载模型后不重启，仅按产品预期操作（返回设置或等待事件），确认 listener 是否自动恢复；若不能，记录为模型生命周期缺口。
- [ ] 连续完成两轮唤醒/正文/结束，验证结束后 listener 自动恢复；超时也应恢复。
- [ ] 其他应用占用麦克风时，错误应归类为 mic busy/permission，不应显示错误的模型下载按钮；关闭语音激活后原热键入口仍可用。

## 9. 证据边界

本报告的“已实现”结论来自功能分支 `9270b43c` 的源码和测试；“当前 beta 未合并”来自 GitHub PR 状态、base/head 元数据和本地 Git 图；“VAD/CPU gate 缺失”是对上述 PCM 调用链的静态审计结论，不是 CPU profiler 结果。Issue 评论中的 Windows 测试包和 Actions 链接属于第一方项目记录，但本次没有下载或运行该产物。
