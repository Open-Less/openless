# Personal assistant replacement contract

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

The local source remains on `appergb/Main-workspace` at the original development baseline with unrelated shared UI changes preserved. Other sessions delivered later general UI work from separate branches; it was not silently merged into this Ask acceptance snapshot.

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
