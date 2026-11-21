# Personal assistant replacement contract

## 延后状态与恢复入口（2026-11-22）

**状态：Deferred / 延后。** 用户决定先处理更重要的功能和严重 bug。本轮只保存已有实现、整理逻辑与提示词缺口；停止 Ask 的新增开发、测试、CI、Beta 合并、打包和发布。恢复开发需要用户重新安排优先级。

- 保存分支：`deferred/ask-anything-20261122`。该分支是开发快照，不是可发布版本。
- 历史保存点：`07353f9c` 保存原 Ask 实现和当时共享现场；`95587040` 合并了较新的通用 UI。此冻结提交另保存审核阶段的 13 个 Ask/Core/native/usage 接点文件及本说明。
- 分支继承了混合开发历史，不能整体当作“仅 Ask 补丁”合入其他分支。后续集成必须重新比较实际差异和共享依赖。
- 由临时独立 Git 索引生成快照；不切换或改写当前 Main 共享 checkout、HEAD、Git 索引或 AOCI，也不收录其他会话之后的严重 bug 修复。
- 本轮没有收录尚未提交的 `scripts/capsule-ready-replay.test.mjs` 与 `src/features/cloud-sync/CloudSyncSection.test.ts` 两项通用测试调整；它们仍留在原工作区，由对应负责人处理。
- 集成负责人给出的不含本轮 Ask 重写的候选基线是当时的 `origin/beta@3798c446`。这只是恢复前的参考，不授权覆盖本地 Beta，也不把已经前进的远端引用视为固定事实。

### 目前写到哪里

| 范围 | 已有实现 | 尚不能据此宣布完成的部分 |
| --- | --- | --- |
| 助手后端 | 独立 Rust `assistant.rs/assistant/`、依赖注入、只读工具循环、取消、最终正文、逐真实调用用量记录，替换旧 Chat 执行链 | 未完成真实供应商与完整 Ask 窗口的集成验收 |
| 四路入口 | 每次提交重新识别普通提问、划词追问、帮我写、语音修改；工具/JSON 路由与可检查的 `editTarget` | “独立模型”目前是独立分类调用，模型工厂仍取当前 LLM 配置；没有独立路由模型配置入口 |
| 前端 | `ask-window/` 的请求归属、工具状态、取消、最终全文收敛、紧凑/展开错误展示，路由互斥设置 | 最新审核修正尚未完成主 Agent 的最终集成复核 |
| 原生写回 | 打开前 A 光标与提交时 B 选区分离；恢复、重验和一次性 claim；失效保留结果 | 完整 GUI → 真实模型 → A/B 写回尚未验证；Windows 支持范围较窄，Linux 原生写回尚不可用 |
| 上下文 | 当前对话、提交的选区/文本/PDF 材料；按授权读取时间、系统、原 App、浏览器与近期语音文字 | `app_purpose` 等字段当前没有可靠来源；历史语音没有原生范围绑定，多轮原图未保留 |
| 预算与联网 | 有效能力窗口内的预算、旧交换/工具证据压缩、公共网页/GitHub Releases、高精度计算、Fake-IP 兼容 | 未知模型使用 16,384 的保守运行上限，不能宣称任意模型都支持 256K；真实供应商压缩语义质量仍待确认 |

审核阶段留下的修正已随分支保存：大工具结果的预算截限、后续追问保留提交的选区/PDF/文本引用、取消时 Reading 来源收敛；新会话和 App 上下文撤权覆盖模型准备期；Windows 前台进程获取的条件编译；共享 usage 通知保持 main-only Host 动作。它们是已有修正的冻结，不是本轮继续实现新功能。

### 逻辑链路与需要守住的边界

1. **打开入口**：在焦点移入 Ask 前保存 A 的位置与授权上下文。胶囊继续作为通用听写，只把文字送入输入框。
2. **显式提交**：以 conversationId/clientRequestId/turnId 和生命周期 fence 绑定本轮；确认当时外部 B 的选区。展开状态不跳过路由。
3. **决定交付位置**：普通提问和划词追问共用助手后端，只回答；帮我写与语音修改单次生成正文，分别交给 A 插入与 B 替换。
4. **模型和工具**：助手只得到受控只读能力。外部材料与历史引用是数据，不能取得写回、系统命令或焦点权限。
5. **回传与结束**：事件按请求与顺序归属，最终正文权威覆盖流式片段；取消/失败后不能复活为成功。已 claim 的原生操作仍须报告真实 Written/Unknown 结果。
6. **原生交付**：生成完再次恢复并验证同一控件和范围；原位置失效不改写到当前碰巧聚焦的编辑器。

以下问题仍应留到恢复时处理，不能只通过追加提示词解决：

- **先查证再写作的复合任务**：当前路由示例将“查汇率、计算、写报价单”归为 write，但 write/edit 的生成路径没有搜索或计算工具。需要决定是否由受控前置取证提供材料，或明确无法完成时的交付；不能让单次正文生成凭空补出实时价格。
- **“刚才那句”的替换**：native preparation 的 recent_voice 当前为空，历史是在 edit 确认后按需读取，且持久语音记录全部 matches_selection=false。现阶段没有“语音记录 → 原 App 控件/范围”的可靠绑定，recentVoice 路由应澄清，不应声称已经完成该能力。
- **独立分类与独立模型的区别**：现有 SeparateModel 再次调用同一 ConfiguredAskModels；语义混合也使用模型输出 JSON。后续应对齐设置名称、模型来源与预期差异，不能用提示词把相同配置描述成独立模型。
- **材料连续性和来源**：审核修正保留提交的选区/PDF/文本；图片仅作为当前轮附件，不能保证后续能重读原图。App 用途和来源 URL 没有实际数据时应保持缺失，不能由提示词推断成已采集。
- **平台与容量**：macOS AX、Windows 标准 Unicode Edit 和其他平台的实际能力不同；未知上下文窗口不是已知有效窗口。恢复时仍需能力数据与真实运行确认。

### 提示词需要加强的位置（本轮只记录，不改行为）

现有基础提示词已经包含自然交流、按需工具、来源与权限规则。以下是下一轮应补充或校准的具体内容，不表示这些边界完全没有实现。

| 优先级 | 位置 | 需要加强的内容 |
| --- | --- | --- |
| 先处理 | `qa_service/standalone/entry.rs` 的 RULES/routing_instructions | 用成对例子覆盖“解释/翻译给我看”与“替换原文”、“写一段”与“怎么写”、有无关选区、跨轮指代、明确留在 Ask，以及复合任务。含糊写入继续 clarify；不能让提示词掩盖 write 无工具或语音绑定缺失 |
| 先处理 | 同文件 ASK_COMMON/execution_rule 与 `standalone/prompt.rs` | 明确正文会立即输入且没有第二轮润色；按真实目标用途决定称呼、长度、格式和语言；默认不包代码围栏、不复述指令/周围文本，必要引用与用户明确格式要求保留；编辑只修改指令要求，保留人名、数字、链接、事实和原意。目标用途缺失时不得假装知道 |
| 先处理 | `assistant/prompt.rs` 的 SYSTEM 与 `assistant/tools.rs` 的工具说明 | 对 `truncated/contextBudget`、部分搜索命中、读取失败/拒绝、来源冲突和重复失败给出明确收敛规则：证据不完整不能声称穷尽或已核实；可以用已有证据完成的部分直接回答，不循环重试同一失败。已有公开检索隐私限制和证据/推断区分继续保留 |
| 随后处理 | `assistant/prompt.rs` 的 COMPACT 与 `assistant/budget.rs` 的摘要约束 | 进一步约束代词的来源和原话、仍有效/已撤回约束、数量/单位/代码符号、精确 URL、读取状态与失败证据。当前逐用户覆盖和原句校验不等于语义上一定无遗漏；用真实供应商确认摘要质量 |
| 随后处理 | SYSTEM 的角色与表达占位符 | 用户确定角色后再补稳定身份、性格、幽默尺度与表达示例；现在保持占位，不擅自编名字/背景/故事。自然聊天不强行工具化，不习惯性反问或生成任务清单 |
| 随后处理 | SYSTEM 的环境/下载建议段及工具说明 | 对“当前网页”与原 App 绑定、授权拒绝、页面不可读、tag/latest/prerelease、ARM/x64/universal 与进程架构的区别给出简短决策例子；只推荐核实存在的资产，不宣称能读取登录态页面或安装软件 |

写回目标校验、权限撤销、取消、模型能力上限和 requestId 归属必须继续由代码硬约束，不能迁移成模型自行遵守的提示词。

### 证据与恢复条件

- 本轮仅做静态阅读、分支快照与文档整理，没有重新运行测试、CI、启动 App 或访问系统授权。
- 审核负责人交接的局部结果为 Core 41 passed / 2 ignored，ask_session 7 passed，ask_native 13 passed / 1 ignored；这些是对方报告的局部证据，主 Agent 尚未完成最终集成复核。
- 下文保留前一阶段的已执行检查和用户人工清单作为历史记录。完整 Ask GUI、真实模型、麦克风和跨平台验收仍有缺口，不能用局部测试或 CI 替代。
- 恢复时先比较届时 Beta、共享上下文/语音接口与本分支的变化；按模块重新认领并解决共享依赖，再执行用户重新授权的检查。暂不指定继续日期或后台任务。


This is the current development contract for the Ask Anything replacement. It supersedes the old standalone contract's single-search restriction, output marker/review pipeline, and rule that an expanded window always bypasses intent routing.

## Ownership

- Lead session `265bc24f-476d-4141-aa48-2dea0d0a28ff`: contracts, integration, frontend coordination, four-route entry orchestration, composition/editing, native targets, authorized context adapters.
- Independent peer Codex session: personal-assistant Rust runtime in `crates/openless-core/src/assistant/` (entry `assistant.rs`), its tools, prompt, model implementation and tests. It does not own the window, dictation, route classifier, composer/editor or native writing.
- Frontend subagent: `ask-window/` and its specific browser tests; errors use the existing notification API.
- Native subagent: `src-tauri/src/ask_window/` and native target checks. Shared core contract changes are coordinated with the lead.
- Core entry subagent: `qa_service/standalone.rs`, `qa_service/standalone/entry.rs`, `prompt.rs`, and lifecycle tests, after the new runtime boundary is ready.

Existing unrelated uncommitted changes belong to the other Main agent. Preserve its usage recorder change notifications, settings changes and translations. Do not change shared Select/Modal/TransitionViewport or settings-shell files concurrently.

## Frontend wire

Keep the actual `ask_submit` command and `ask:event` channel; the frontend contract is `ask-window/client.ts`.

Request: `{conversationId, clientRequestId, text, attachmentIds, options:{deepThinking}}`. Input method is not a route or tool-permission signal. The host prepares authorized context and native targets at submission; native pointers, control identifiers and write leases never enter this payload or a model prompt.

Acceptance returns `{turnId}`. `clientRequestId` identifies the submission; repeating identical content returns the same turn, while changed content with the same identifier is rejected. Events and replay carry `conversationId`, ordered `sequence`, and the applicable `turnId`. The frontend reconciles accepted requests before expanding and rejects events from an older conversation/turn/visibility generation.

Existing events remain `submitted`, `routeResolved`, `turnAccepted`, `answerDelta`, `turnStatus`, `sourceChanged`, `turnFinished`, `delivery`, and attachment events. General tool state adds `toolChanged` with `{turnId, toolCallId, name, status, error?}`; status is `running|completed|failed|cancelled`. Only public tool status and source metadata cross to the UI, not raw private evidence.

`ask_cancel(conversationId, turnId)` cancels that request and its tool work. Final state is exactly one of `completed|cancelled|failed`; a stopped request cannot later publish a completed answer or write text. Failure is a typed error code/reason; frontend maps it to a stable user-facing error code and localized explanation. Raw provider errors are never displayed in the composer.

Expanded errors use the existing top notification API, with no Ask/error title or inline error block. Compact errors briefly shake the entire compact window and use its existing placeholder area for “出现了一点错误 · 错误代号”, without resizing the window or overwriting the draft.

## Rust boundary

The executable boundary is `openless_core::assistant_api`:

- `AssistantRuntime::run(AssistantRequest, AskCancellation, AssistantEventSink)` serves only `answer` and `selectionAnswer`.
- Requests carry scoped IDs, the current instruction, live conversation messages, optional quoted selection/source/context, validated attachment content and thinking choice.
- `AssistantEvent` reports text deltas, thinking/writing/compacting state, actual tool state, sources and usage.
- Successful `AssistantResponse` is the complete final answer and ends the request. No mandatory result marker, no secondary model review/polish, and no fixed-prefix validation decides whether natural chat is usable.
- `AssistantContextProvider::read` exposes typed read-only queries for current time, system, original foreground app, browser tab and bounded relevant voice history. The native adapter owns authorization and reports available/unavailable/denied explicitly.
- The runtime does not receive `AskWriter`, clipboard access or a native focus target.

The peer chooses runtime internals and constructor dependencies after inspecting existing model/search/context APIs. It coordinates additions to this shared contract with the lead before editing shared files. Existing `ask_search`, attachment validation, credential/preferences services, HTTP safety and the common usage recorder are candidates for reuse, not permission to keep the old chat runtime.

## Entry and native delivery

The entry orchestrator classifies every explicit submission into `answer`, `selectionAnswer`, `write`, `edit`, or a clarification outcome. Punctuation or the presence of a selection alone cannot decide the route.

`answer` and `selectionAnswer` delegate exclusively to the new assistant runtime. `write` and `edit` use a separate one-pass final-text generation path, then the host's native delivery; neither calls the assistant's tool loop nor a second polishing/review model.

Keep original A caret captured before opening separate from B selection verified at submission. `write` targets A, `selectionAnswer` only quotes B, `edit` only replaces B. Revalidate the bound control and range before writing; preserve generated text when invalid or uncertain. Never fall back to the currently focused arbitrary editor.

Dictation remains ordinary text input. Auto-start controls whether opening starts the capsule, not whether transcription submits a request. Current conversation memory works when persistent chat history is off. Voice transcriptions are separately owned by the context/editing side and queried only when relevant and authorized.

Browser tab and recent voice text have independent, default-off grants in `ask-read-context.json`, read and saved through main-window-only `ask_get_read_permissions` / `ask_set_read_permissions`. The older desktop-app-name grant does not enable either. Revocation cancels active work and invalidates in-flight reads. Browser native process identities stay local; `AssistantContextResult` distinguishes an unavailable API, missing Accessibility permission, and an ambiguous page.

The macOS browser adapter reads only the original browser's focused page title and HTTP(S) URL through Accessibility; public page content uses the read-only web API. Safari, Chrome/Chromium, Edge, Brave, Vivaldi, Arc, Orion and Ego Lite have declared bundle support, subject to the browser actually exposing an unambiguous web area. Other platforms currently return `browserTabUnsupported`. System metadata includes the OS version and architecture; macOS distinguishes the host's ARM capability from the process architecture. Windows native write support remains restricted to verified standard Unicode Edit controls, as documented in the native adapter contract.

Recent voice data comes from the independent history API, limited to 30 minutes, at most eight relevant successful dictation text/time records. Editing requests ask for at most four records related to the verified selection. Audio, ASR state, persistent chat rows and native handles are excluded. Durable text equality cannot prove an editor/range binding, so these rows have `matchesSelection=false`; a `recentVoice` replacement requires a separately verified native binding or returns clarification.

Public page reading keeps private/network-address blocks and pins validated public DNS addresses. With the user's system-proxy policy enabled, an all-benchmark Fake-IP DNS response can be resolved through the fixed Cloudflare HTTPS DNS endpoint, after which the returned real addresses undergo the same public-address checks. The resolver receives only the target hostname and DNS record type. Page reading remains anonymous, rejects redirects and non-web ports, and has a 15-second deadline.

## Acceptance

Required runtime acceptance uses two real editors: A caret → open Ask → select B; compose writes to A, selection question leaves B unchanged, editing replaces B only. Also verify dictation fills without submitting; tool request completion/cancellation; stale events; transient conversation memory; compact versus expanded errors. Report platform gaps and any real execution that was not possible separately from compilation or mocked tests.

### Verification status, 2026-11-21

- Core entry regression: 20 tests passed, covering all routes, no second review call, ordinary questions excluding incidental selections, verified voice-edit scope, scoped events, cancellation, usage and transient dialogue.
- Native Ask regression: 34 tests passed. Standalone production-context/wire tests: 13 passed; the separate controlled-browser live test also passed.
- The ignored macOS two-editor test ran against the real `TargetTracker` and two disposable native `NSTextView` windows. Original A caret insertion, B-only replacement of repeated text, document/range invalidation, and cancelled claims passed. It exposed and fixed a nil App metadata read in `capture::app_identity`. This verifies the native target workflow, not the complete Ask GUI/model workflow.
- Ask frontend: the targeted WebKit runs passed, including the final seven error/tool/final-text scenarios. Chinese, English and expanded German routing labels were checked at 1000px without wrapping, clipping or control overlap. Existing cross-browser results from the frontend owner remain applicable to unchanged paths.
- The independent backend session reported passing real local HTTP tool-loop tests, four-protocol model regressions, usage/cancellation and context-compaction tests, including older user input and tool-work compression. Real anonymous GitHub release metadata and real public-page reading passed. The latter required the Fake-IP compatibility change and the realistic 15-second reading deadline.
- `tauri build --debug --bundles app --ci` produced a local debug App. Its startup reached macOS SecurityAgent authorization; the computer-control tool refuses access to `com.apple.SecurityAgent`. The user subsequently directed the agents to stop further testing and leave manual acceptance to the user. Real provider calls and the full Ask GUI → model → native A/B workflow, plus live microphone transcription, remain unverified. Windows/Linux runtime acceptance has not run.

The verification list above records the earlier acceptance snapshot. A later review candidate merged the newer general UI and retained additional corrections; the 2026-11-22 deferred status at the top is authoritative. The full native/provider acceptance remains unfinished.

### 用户手动验收清单

本清单由用户稍后执行。Agent 不再启动测试或等待系统授权。使用本轮 Ask 验收 App：`openless-all/app/src-tauri/target/debug/bundle/macos/OpenLess.app`。另一个会话交付的通用 UI DMG 不包含本轮 Ask 重写。

准备两个临时的「文本编辑」纯文本窗口，避免使用重要文档。A 输入 `A🙂 前文 后文`，光标放在「前文」后；B 输入 `B🙂 red red end`，用于选择第二个 `red`。前三个场景先关闭「自动开始语音输入」，用打字提交；每个场景前关闭 Ask、恢复两份原文，再从 A 的光标打开 Ask，之后去 B 选择第二个 `red`，最后回到 Ask 提交。

| 场景 | 操作 | 通过标准 |
| --- | --- | --- |
| 帮我写回 A | 提交「帮我写一句周末祝福，直接写入原应用」 | 正文只插入 A 的原光标处；B 不变；没有开场白、解释或第二次润色。 |
| 划词追问 B | 提交「选中的词是什么意思？只在这里回答」 | 展开聊天解释 B 的选区；A、B 都不变。再问「日本首都是哪里」，即使 B 有选区，也应普通回答。 |
| 语音修改 B | 提交「把选中的 red 改成 blue，替换原文」 | 只有 B 的第二个 red 变成 blue，第一处 red 和 A 均不变。带问号的「能把选中这段改短吗？」也应按修改处理。展开聊天后再提交明确修改要求，也要重新路由。 |
| 目标失效与取消 | 生成期间修改目标文档、移动 B 的选区或关闭目标窗口；另一次在处理时按 Esc | 失效时不写其他控件，结果保留供复制。取消后停止处理，不出现迟到回复或写入，也不把 Esc 当答案。 |
| 胶囊只听写 | 开启「自动开始语音输入」，从外部编辑器打开 Ask，说一段话后结束录音 | 文字只进入当前 Ask 输入框，不自动提交。手动提交后行为与打字一致。关闭自动语音后，打开 Ask 不连带录音。中文输入法选词回车不应直接提交。 |
| 真实工具 | 问「现在本机日期时间是什么」；在已授权的支持浏览器打开 GitHub Releases，问「这个项目我该下哪个安装包」；要求用高精度工具计算 `12345678901234567890 + 1` | 时间与系统一致；推荐实际存在且匹配当前系统/架构的发布资产并给出来源链接；不会下载或安装。计算结果为 `12345678901234567891`。工具处理能结束，也能取消。 |
| 当前对话与权限 | 先关闭保存聊天历史，再在同一窗口说「测试代号是蓝鲸」，随后问代号；关闭重开后再问；分别开关浏览器与近期语音文字授权 | 当前窗口能引用前文；关闭重开后不串入旧对话。关闭聊天历史不等于授权语音历史。撤销环境/语音授权后不继续读取。无可靠选区时要求修改「刚才那句」，不能猜位置覆盖原文。 |
| 两种错误界面 | 紧凑时取消外部选区，提交「替换选中文字」；已展开聊天后，同样在无选区时发修改要求 | 紧凑时整块窗口轻晃，原占位区域短暂显示「出现了一点错误 · ASK-代号」，草稿保留、继续编辑后恢复，窗口不撑高。展开时仅用顶部统一弹窗提示；无 Ask/原始错误标题、输入框内错误段落或旧核验折叠框。代号见 `docs/ask-error-codes.md`。 |
| 路由设置 | 在「高级功能 → 意图路由」切换三种模式，再关掉已启用的 Beta 模式并重启 | 默认独立模型 Beta；三种模式互斥；两个 Beta 都关时回到语义混合；保存后重开仍一致；保存期间相邻开关不变淡。 |
| 长对话，可后测 | 在对话开头给出几条明确约束和目标，持续多轮较长交流，再引用最早目标与约束 | 接近所选模型的有效窗口时可整理旧上下文，随后仍保留有效约束和必要来源，不凭空改变任务。真实供应商的长对话摘要质量尚未实测；触发点受模型窗口影响，不是每个模型都能使用 256K。 |

发现问题时，记录所用 App/模型、输入原句、从 A 打开还是从 B 打开、提交时选区、错误代号，以及是否已展开。截图或短录屏应覆盖提交前后和实际写入位置，便于区分路由、模型结果、展示与原生目标问题。
