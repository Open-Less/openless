# 录音采集与后台后处理

**2026-11-23 规则更新：普通听写新增文字向最终交付时的当前焦点输入，不冻结原输入框或光标，也不激活旧应用。开始时的应用、场景和风格仍冻结，用于请求路由。只有选区润色、翻译和修改既有内容的 voice-edit 才复核原目标、范围及原文。下文旧的普通听写范围限制已被本段覆盖；真机验收与受控回归分别记录。**

普通听写在宿主确认录音设备停止后允许开始下一段。后台段复用原 `sessionId`，继续完成转写、润色、插入及历史写入。麦克风同一时刻仍只有一个采集拥有者。

## 接口与所有权

| 接口或事件 | 含义 |
| --- | --- |
| `BackendSnapshot.dictation.sessionId` | 当前采集拥有者；包括 Starting、Recording，以及等待设备 stop 的短暂阶段。设备未确认停止时仍不能开始下一段。 |
| `BackendSnapshot.processingDictations` | 按提交时间排列的后台任务状态，最早提交的在前。每项仍有原 sessionId、阶段及错误消息；不代表麦克风占用。 |
| `submit_dictation_session(&Arc<Self>, id, stopOptions)` | 将完整收尾交给 Core 拥有的任务，确认设备停止并提交后返回该 id。调用窗口关闭或 IPC 回复被丢弃，不会丢掉后续结果和历史。 |
| `stop_dictation*` | 保留等待本段最终结果的旧语义。实际设备已释放时，这个等待不再占用采集位置。 |
| `wait_for_dictation_submission(id)` | 只在设备 stop 已加入、任务已登记后完成。宿主必须与最终结果同时等待，处理启动失败或提交前取消。 |
| `submitted_dictation_context(id)` | 返回本段最终、只读的 `Arc<DictationContext>`。这是停止时确定的模式、翻译语言、风格、识别/润色渠道、凭据、热词、纠错规则、上下文和目标类型。 |
| `DictationStateChanged`、`DictationCompleted` | 继续按事件原有 sessionId 归属。收到旧段事件不等于当前录音发生变化。 |
| `DictationJobsChanged` | 在设备释放并提交、任务清理时发布完整后台列表。提交事件的 sessionId 仍在列表中；清理事件的 sessionId 已离开列表。 |
| `cancel_dictation(Some(id))` | 定向取消采集段或后台段，仅清理其音频、识别器、插入资源和历史草稿。 |
| `cancel_dictation(None)` | 只取消当前采集拥有者；没有采集时不会取消某个任意后台段。UI/CLI/Esc 先用 `dictation_cancel_target()` 解析明确 id：有采集选采集，否则选最早后台任务。 |

普通桌面 stop/cancel IPC 现在接收可选的 sessionId，原生胶囊与 React 胶囊的按钮都传递明确身份。新录音上方的处理指示器取消最早后台任务，下方取消按钮只取消当前段。原生 helper 也核对显示帧携带的后台 id；过期按钮不能取消后来显示的任务。

翻译复用 `get_dictation_translation_target`、`select_dictation_translation_target(sessionId, language)` 与 `toggle_dictation_translation_for_session(sessionId)`，只允许当前采集段改变。内置 Shift 现在识别完整独立短按，并按通用设置的次数双向切换；自定义翻译快捷键保留原开启入口，见 [Shift 翻译契约](shift-translation.md)。在 stop 将阶段切到 Transcribing 后，迟到的语言选择、Shift 或新设置不能改变本段快照。前台应用及输入上下文继续在开始时捕获，最终模式/目的地与语言选择在停止提交前封存。

音频和历史文件沿原 sessionId 命名。音频生产者先停止并加入，缓冲区和归档仍属于原 PipelineSession；新段创建新缓冲区。普通听写当前没有截图、剪贴板正文或文件附件参数，本次没有新增隐式读取；后继功能接收这些素材时必须放进本段冻结上下文，不能在后台读取全局“当前素材”。

## 后处理与写回

Core 用同一条有序处理路径串行完成最终 ASR、润色与写入。是否等停止后才连接 ASR 由本段冻结的“稳定转写”偏好决定，不因本地渠道统一强制开启；支持实时输出的本地模型继续在录音期间返回胶囊原文。TextStream 始终保留既有实时协议，SenseVoice/Gemma、Foundry 等离线适配仍按自身能力返回终稿，不因此获得流式识别能力。

采集和本地推理分别拥有资源：`PipelineDictationEngine` 在麦克风已打开后通过宿主 task spawner 启动本地 ASR；模型加载或共享执行器等待期间，现有有界 PCM 缓冲保留音频。Sherpa 在线模型的每段仍有独立 `OnlineStream` 和 worker 队列，同一个缓存 recognizer 的建流、PCM 解码、分段、最终 flush 与销毁通过同一个执行锁串行；锁不覆盖整段会话寿命，也不覆盖润色或设备采集。后段可先采集并排队音频，前段解码让出执行器后继续输出本段原文。Sherpa 离线解码仍使用已有 permit，Foundry 仍使用已有 lifecycle/route 保护；实验性离线模型在原 `work` 锁上等待前次推理，而不是对重叠启动直接返回 Busy。旧 Sherpa 会话取消只清理自己的 worker/音频，不再设置运行时全局 prepare 取消标记。

桌面原生键盘/输入源准备延后到最终写入，录音阶段不持有另一段的输入源或组合文本。胶囊流式原文保留；桌面目标输入框的润色文字在处理完成后交付，不在录音期间持续写入。其他宿主可以继续使用原有的准备时机，Core 的麦克风与后台资源仍分开登记。

普通听写仍通过 CapturedTextTarget 捕获提示上下文，但 Tauri 插入适配器在最终处理队列内才确定实际输入目标。普通听写不调用旧范围拒写策略，不为完成旧段拉回焦点。Core 对延迟准备的插入资源关闭目标实时组合与分块写入；胶囊 ASR partial 保持实时。TSF、Unicode 与粘贴最终提交都留在同一最终处理锁内。选区服务仍沿既有 SelectionInsertionTarget、完整原文与范围验证执行，不受此策略变更影响。

安全写回的结果沿现有结果/错误类型返回：

- `InsertOutcome::Inserted`：宿主确认插入；`PasteSent`：已发出粘贴但没有确认结果；`CopiedFallback`：仅复制，不能显示成已插入。
- 准备前目标/范围变化或无法验证返回 `BackendErrorCode::InvalidState`，同时使用既有 `target_unavailable()` / `is_target_unavailable()` 标记，该路径确认没有开始写入；本段转写与润色正文保留在历史，错误码为 `insertTargetChanged`。终态消息为 `input_target_changed`，桌面显示“输入位置已变化或无法确认，未自动输入；结果已保留在历史记录”。这与 `OutcomeUnknown` 明确区分。
- 已派发且无法确认的原生写入返回 `BackendErrorCode::OutcomeUnknown`，历史记录为 `insertOutcomeUnknown`。不能自动重试，以免重复插入。
- 其他写入失败仍为 `insertFailed`。不把这些错误自动转交给新的录音目标。

## 竞态选择

| 情况 | 最简安全行为 |
| --- | --- |
| 同一输入框连续两段 | 录音可以接续；前段完整交付后，后段才开始实际输入。 |
| 普通听写处理中移动光标或切换输入框 | 最终向当前焦点输入；开始时捕获的应用仍决定本段提示与风格。 |
| 两段来自不同应用 | 不抢焦点。每段轮到实际交付时向当前焦点输入，模型上下文各自冻结。 |
| 处理中原窗口/输入框失效 | 拒绝向当前焦点写入，保留该段历史正文。控件身份不可验证也按此处理。 |
| 第一段成功、失败或取消时第二段仍在录音 | 只结算第一段；第二段麦克风、首尾帧、热键代次、静音检测、胶囊原文及取消令牌不变。迟到的旧录音故障同样拒绝。 |
| 第一段很慢，第二段已完整录完 | 两段都在后台列表内，麦克风空闲，可以继续采集；第二段排队，不运行第二份不安全的本地模型。 |
| 完成先后与提交先后 | 最终处理串行化，因此结果按提交顺序完成；取消某段可以提前终止它，但不会使其他段失效。 |
| 旧引擎不支持确认设备停止或 stop 报错 | 保持旧等待语义，直到其清理完成才释放采集位置，不能猜测设备已释放。 |

## 胶囊位置

Classic 当前录音胶囊固定在宿主底部并位于前景；上一段处理胶囊向上移入后层，向右错开 10px，40px 高度中下半部被前景遮住，上半部保留可读的“正在转录”。它复用现有材质与圆角，重叠轮廓连成一体，宽度随多语言标签和数量适配，不再使用独立处理球体。处理数量变化不参与原文、翻译徽章或菜单的排版。Classic 原文轨道与身体仍固定保留 24px；语言菜单沿用宿主向上扩窗规则，没有第二个处理悬浮窗口。

基线 `653816ac` 的启动入口固定调用 `desktop_resources::configure(..., false)`，因此默认桌面胶囊是 Tauri WebView，macOS 使用 WKWebView；`capsuleStyle=classic` 只选样式，不代表启用独立 egui helper。默认路径和备用 `openless-capsule` renderer 都消费原 sessionId 与 processing snapshot，后台翻译直接使用已有 `translationActive`，不读取下一段的模式或目标。

翻译角标在 recording、transcribing、polishing/inserting 期间保留“正在翻译”；采集期间保留语言选择，后台阶段只展示状态。旧段翻译与新段普通录音并存时，角标保留直到后台列表移除该段。原文出现引起角标换位置时，React 的同一 motion 元素直接动画 bottom，原生 helper 的 Tween 从当前值改目标，不卸载重建。原文轨道与其他页面不因处理胶囊出现而重新排版。

`empty_transcript` 由宿主映射为“未识别到语音，录音已保留，可在历史中重新转录”，React 与 helper 按界面语言显示，不再显示输入或设置无效；当前录音期间旧段错误继续沿宿主已有 session 归属过滤。

后台语义事件不能改写新录音的原文身份或 native capsule 的显示身份。任务列表变化只更新处理指示器；仅设备释放事件允许从录音转入后台显示。其他段的成功/错误回执、复制回退卡不会盖过当前录音。

## 验收证据与平台边界

| 要求 | 实际入口 / 源码 | 验证与限制 |
| --- | --- | --- |
| 普通热键、按钮、Core stop/start | `src-tauri/src/coordinator/dictation_core.rs`、`commands/dictation.rs`；Core `api.rs` 的 `submit_dictation_session` 和 stop 公共路径 | 保留热键按序受理直到设备释放，后处理由独立任务继续；Core 相关 API 回归覆盖旧入口。 |
| 独占采集、逐段缓冲、延迟成功/失败/取消 | `voice_session.rs`、`dictation_engine.rs`、`recording_overlap_tests.rs` | 用真实 Pipeline/Router 和可控音频夹具检查六种交错：第二段继续采集/已完整录完 × 第一段成功/失败/取消；验证每段全部首中尾帧、历史 id、一个采集 owner，以及迟到旧录音故障隔离。不是物理麦克风实测。 |
| 本地实时原文及非重入执行 | `recording_overlap_tests.rs`、`src-tauri/src/asr/local/sherpa_runtime.rs`、`backend-tests/tests/sherpa_online.rs` | 同一生产 start/stop/finish 路径分别覆盖实时/稳定偏好下的上述六种交错，实时段 stop 前必须收到同 session partial，前段延迟 native 操作时后段采集可启动、解码等待，终稿/取消/历史不串段。独立宿主夹具包含生产 Sherpa 执行锁、旧段取消和离线超时所有权测试；默认使用非 native facade，不代表真实模型、物理麦克风或 Windows/Android 设备验收。 |
| 请求使用冻结上下文 | `recording_overlap_tests.rs` → 生产 `providers::OpenAiChatPolisher` → 本机 HTTP 夹具 | 检查真实序列化的 system/user 消息与各段冻结上下文完全相同，包含捕获的光标材料；另行核对处理中改变全局语言不改变旧段语言快照。没有调用真实云模型。 |
| 流式原文、语言菜单、多任务上下叠放 | `e2e/capsule-overlap.spec.ts`、`src/lib/capsuleLayout.test.ts` | Chromium 使用生产 React 组件和隔离宿主事件，核对固定身体位置、后景偏移 10px 与 20px 半遮、语言菜单开闭、流式原文、1→3→1 后台数量及定向取消；截图由测试输出。 |
| native 事件与低内存胶囊 | `src-tauri/src/capsule_snapshot.rs`、`crates/openless-capsule`、`native-capsule` | native snapshot 的无 Tauri 回归、renderer/协议测试及 macOS 编译检查。没有宣称原生窗口在物理桌面实测。 |
| 可信原目标与位置 | `selection.rs`、`selection/target_position.rs`、`host_document/{macos,windows}.rs`、`core_adapters.rs`；Core `tests/dictation_target_capture.rs` | 数值范围校验复用生产函数，在 Core 收尾夹具复现同控件移光标、移光标后第二段、同位置第二段、范围不可读四种情形，验证旧段零写入、历史正文/明确未插入状态和第二段独立交付；不是实际桌面控件实测。macOS 编译覆盖；Windows 分支需 Windows 构建/真机验收。 |
| 其他平台 | 共享 Core 同一采集/任务协议 | Linux、Android、HarmonyOS、iOS 没有在本机进行编译或设备验收；各自专用 UI 尚未增加桌面上下处理球体。不要把桌面 React/原生 helper 的证据外推到这些 UI。 |

在应用目录可运行受影响检查：`cargo test --locked -p openless-core --lib api::tests::`、`dictation_engine::tests::`、`voice_session::tests::`，`cargo test --locked -p openless-core --test contract_2`，`cargo test --locked -p openless-capsule --features ui,host --lib`，以及 `cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml --test capsule_snapshot`。UI 使用 `OPENLESS_E2E_PREVIEW_PORT=5511 OPENLESS_E2E_FIXTURE_PORT=5512 npx playwright test e2e/capsule-overlap.spec.ts --project=chromium`，不使用用户的 5435 预览。

## 风格组合基线与素材复核

本分支以 merge 保留 `30867f75` → `3330347d` 的风格祖先，仍由 `DictationContext::capture` 调用 `style_packs::dictation_style_prompt(style_pack)` 初始化风格；位置加固没有改动风格逻辑。组合验收只补 `dictation_target_capture`、`prompt_contract`、Raw 生产 Pipeline 零润色调用、双段隔离及宿主编译；真实模型和旧版同步仍由对应独立验收负责。

复核普通入口 `commands/dictation.rs` → `DictationStartOptions` → `capture_dictation_context_with_host` → `DictationContext` → `PipelineDictationEngine` / `DictationEngineRouter`：可接受材料只有显式前台应用/光标上下文、词汇/纠错规则和开始时捕获的历史轮次，均在段内快照中克隆保存；最终模式/语言在 stop 时封存。`DictationInsertionContext` 的剪贴板字段只控制结果复制/恢复，不接收剪贴板正文。问答、选区与其他服务有各自输入，不能把那些服务的全局或临时材料冒充为普通听写的段内附件。本次未找到普通听写可直接复用的截图、剪贴板正文或文件附件提交参数，没有新增附件系统，也没有宣称附件归属已经实测。


### 2026-11-23 叠放与翻译角标受控验收

`e2e/capsule-overlap.spec.ts` 在 Chromium 与 WebKit 检查叠放、菜单、流式原文、原 session 取消、旧段成功/失败/取消、新段继续录音、后台翻译角标与空稿提示。动画测试逐帧记录同一 DOM 元素的位置，要求存在中间位置且没有一次完成全部位移。

`scripts/capsule-stacking-webkit.swift` 用独立 macOS WKWebView 加载同一生产组件夹具，输出原生 WebKit 截图和逐帧坐标。先启动隔离 Vite，再用 `swiftc` 编译并传入夹具 URL 和已有输出目录；它不启动安装版 OpenLess，不使用麦克风或实际落字，也不验证 Tauri 热键与窗口宿主。

本机受控截图的 172px 局部内，前景为 `(172,124,116,32)`，后景为 `(188,104,104,40)`；原文轨道、翻译角标在后台任务出现/移除和开关菜单时坐标不变。翻译流式切换另行记录同一角标连续移动的中间帧。历史页面未改动，不把本次胶囊宿主截图当作历史弹层真机验收。
