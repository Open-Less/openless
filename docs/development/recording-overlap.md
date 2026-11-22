# 录音采集与后台后处理

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

翻译仍复用 `get_dictation_translation_target`、`select_dictation_translation_target(sessionId, language)` 和现有 Shift 意图入口，只允许当前采集段改变。在 stop 将阶段切到 Transcribing 后，迟到的语言选择、Shift 或新设置不能改变本段快照。前台应用及输入上下文继续在开始时捕获，最终模式/目的地与语言选择在停止提交前封存。

音频和历史文件沿原 sessionId 命名。音频生产者先停止并加入，缓冲区和归档仍属于原 PipelineSession；新段创建新缓冲区。普通听写当前没有截图、剪贴板正文或文件附件参数，本次没有新增隐式读取；后继功能接收这些素材时必须放进本段冻结上下文，不能在后台读取全局“当前素材”。

## 后处理与写回

Core 用同一条有序处理路径串行完成最终 ASR、润色与写入，避免为了重叠录音而同时调用不安全的本地模型。本地 ASR 使用已有“停止后识别”的缓冲能力；显式实时 TextStream 保留原实时协议。云端 ASR 仍可在录音期间返回胶囊原文。

桌面原生键盘/输入源准备延后到最终写入，录音阶段不持有另一段的输入源或组合文本。胶囊流式原文保留；桌面目标输入框的润色文字在处理完成后交付，不在录音期间持续写入。其他宿主可以继续使用原有的准备时机，Core 的麦克风与后台资源仍分开登记。

目标继续使用 `CapturedTextTarget` 和 `selection::SelectionInsertionTarget`，没有第二套目标系统。普通听写在同一目标内冻结原光标/选区的 UTF-16 范围，并记录输入目标的采集代次；另一段开始捕获目标时，旧段自动写回资格即失效，即使控件和光标位置碰巧相同也不恢复。准备与最终交付均沿 `selection_insertion_target_is_current` 检查这个范围和代次，不复原、猜测或追踪新光标。

macOS 目标包括进程、窗口、保留的 AX 输入控件身份，并复用 `host_document` 原有 gated AX 范围读取；只读 `AXSelectedTextRange`，不读文档或选区正文。Windows 在原窗口/线程指纹上复用 UIA runtime ID，并用原生 Edit/RichEdit 的 `EM_GETSEL` 读取准确范围。**Windows 浏览器及其他自定义控件目前没有可靠的无正文范围读取：明确拒绝自动输入，结果留在历史，不把 UIA 控件身份当成光标证明。** 不支持范围、范围变化、窗口关闭、切到其他应用或同窗口其他控件时都安全拒绝。

普通听写目标采用一次最终交付，关闭目标输入框的分块写入，不为自己的前缀猜测新的合法范围；胶囊里的流式原文保持不变。不会主动激活旧应用。长选区可复用同一个 `SelectionInsertionTarget`、现有 `validate_selection_insertion_target` 及其原文复核流程；本次只加固普通听写位置边界，没有修改长选区的范围确认逻辑。

安全写回的结果沿现有结果/错误类型返回：

- `InsertOutcome::Inserted`：宿主确认插入；`PasteSent`：已发出粘贴但没有确认结果；`CopiedFallback`：仅复制，不能显示成已插入。
- 准备前目标/范围变化或无法验证返回 `BackendErrorCode::InvalidState`，同时使用既有 `target_unavailable()` / `is_target_unavailable()` 标记，该路径确认没有开始写入；本段转写与润色正文保留在历史，错误码为 `insertTargetChanged`。终态消息为 `input_target_changed`，桌面显示“输入位置已变化或无法确认，未自动输入；结果已保留在历史记录”。这与 `OutcomeUnknown` 明确区分。
- 已派发且无法确认的原生写入返回 `BackendErrorCode::OutcomeUnknown`，历史记录为 `insertOutcomeUnknown`。不能自动重试，以免重复插入。
- 其他写入失败仍为 `insertFailed`。不把这些错误自动转交给新的录音目标。

## 竞态选择

| 情况 | 最简安全行为 |
| --- | --- |
| 同一输入框连续两段 | 第二段开始捕获目标后，第一段不再自动写入，即使两段原光标位置相同。第一段结果留在历史；第二段仅在自己的原范围仍可验证时自动交付。不会把第一段写到第二段的新光标。 |
| 第一段处理中仅在原控件内移动光标/改变选区，没有第二段 | 原范围比较失败，第一段不自动写入，结果留在历史；控件身份相同不足以允许落字。 |
| 两段来自不同应用 | 不抢焦点。第一段完成时若原目标不在前台，保留历史正文并报告未插入；第二段只使用自己的目标。 |
| 处理中原窗口/输入框失效 | 拒绝向当前焦点写入，保留该段历史正文。控件身份不可验证也按此处理。 |
| 第一段成功、失败或取消时第二段仍在录音 | 只结算第一段；第二段麦克风、首尾帧、热键代次、静音检测、胶囊原文及取消令牌不变。迟到的旧录音故障同样拒绝。 |
| 第一段很慢，第二段已完整录完 | 两段都在后台列表内，麦克风空闲，可以继续采集；第二段排队，不运行第二份不安全的本地模型。 |
| 完成先后与提交先后 | 最终处理串行化，因此结果按提交顺序完成；取消某段可以提前终止它，但不会使其他段失效。 |
| 旧引擎不支持确认设备停止或 stop 报错 | 保持旧等待语义，直到其清理完成才释放采集位置，不能猜测设备已释放。 |

## 胶囊位置

默认胶囊下方身体仍固定在宿主底部，顶部独立定位一枚 54×20 的处理球体并连接到身体。处理数量变化不参与正文、翻译徽章或菜单的排版。Classic 原文轨道与身体之间固定保留 24px；React、Windows 外置轨道定位和低内存原生 helper 使用相同间距。语言菜单仍使用现有宿主向上扩窗规则；没有创建第二个处理悬浮窗口。

后台语义事件不能改写新录音的原文身份或 native capsule 的显示身份。任务列表变化只更新处理指示器；仅设备释放事件允许从录音转入后台显示。其他段的成功/错误回执、复制回退卡不会盖过当前录音。

## 验收证据与平台边界

| 要求 | 实际入口 / 源码 | 验证与限制 |
| --- | --- | --- |
| 普通热键、按钮、Core stop/start | `src-tauri/src/coordinator/dictation_core.rs`、`commands/dictation.rs`；Core `api.rs` 的 `submit_dictation_session` 和 stop 公共路径 | 保留热键按序受理直到设备释放，后处理由独立任务继续；Core 相关 API 回归覆盖旧入口。 |
| 独占采集、逐段缓冲、延迟成功/失败/取消 | `voice_session.rs`、`dictation_engine.rs`、`recording_overlap_tests.rs` | 用真实 Pipeline/Router 和可控音频夹具检查六种交错：第二段继续采集/已完整录完 × 第一段成功/失败/取消；验证每段全部首中尾帧、历史 id、一个采集 owner，以及迟到旧录音故障隔离。不是物理麦克风实测。 |
| 请求使用冻结上下文 | `recording_overlap_tests.rs` → 生产 `providers::OpenAiChatPolisher` → 本机 HTTP 夹具 | 检查真实序列化的 system/user 消息与各段冻结上下文完全相同，包含捕获的光标材料；另行核对处理中改变全局语言不改变旧段语言快照。没有调用真实云模型。 |
| 流式原文、语言菜单、多任务上下叠放 | `e2e/capsule-overlap.spec.ts`、`src/lib/capsuleLayout.test.ts` | Chromium 使用生产 React 组件和隔离宿主事件，核对固定身体位置、球体居中与 2px 连接、语言菜单开闭、流式原文、1→3→1 后台数量及定向取消；截图由测试输出。 |
| native 事件与低内存胶囊 | `src-tauri/src/capsule_snapshot.rs`、`crates/openless-capsule`、`native-capsule` | native snapshot 的无 Tauri 回归、renderer/协议测试及 macOS 编译检查。没有宣称原生窗口在物理桌面实测。 |
| 可信原目标与位置 | `selection.rs`、`selection/target_position.rs`、`host_document/{macos,windows}.rs`、`core_adapters.rs`；Core `tests/dictation_target_capture.rs` | 数值范围校验复用生产函数，在 Core 收尾夹具复现同控件移光标、移光标后第二段、同位置第二段、范围不可读四种情形，验证旧段零写入、历史正文/明确未插入状态和第二段独立交付；不是实际桌面控件实测。macOS 编译覆盖；Windows 分支需 Windows 构建/真机验收。 |
| 其他平台 | 共享 Core 同一采集/任务协议 | Linux、Android、HarmonyOS、iOS 没有在本机进行编译或设备验收；各自专用 UI 尚未增加桌面上下处理球体。不要把桌面 React/原生 helper 的证据外推到这些 UI。 |

在应用目录可运行受影响检查：`cargo test --locked -p openless-core --lib api::tests::`、`dictation_engine::tests::`、`voice_session::tests::`，`cargo test --locked -p openless-core --test contract_2`，`cargo test --locked -p openless-capsule --features ui,host --lib`，以及 `cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml --test capsule_snapshot`。UI 使用 `OPENLESS_E2E_PREVIEW_PORT=5511 OPENLESS_E2E_FIXTURE_PORT=5512 npx playwright test e2e/capsule-overlap.spec.ts --project=chromium`，不使用用户的 5435 预览。

## 风格组合基线与素材复核

本分支以 merge 保留 `30867f75` → `3330347d` 的风格祖先，仍由 `DictationContext::capture` 调用 `style_packs::dictation_style_prompt(style_pack)` 初始化风格；位置加固没有改动风格逻辑。组合验收只补 `dictation_target_capture`、`prompt_contract`、Raw 生产 Pipeline 零润色调用、双段隔离及宿主编译；真实模型和旧版同步仍由对应独立验收负责。

复核普通入口 `commands/dictation.rs` → `DictationStartOptions` → `capture_dictation_context_with_host` → `DictationContext` → `PipelineDictationEngine` / `DictationEngineRouter`：可接受材料只有显式前台应用/光标上下文、词汇/纠错规则和开始时捕获的历史轮次，均在段内快照中克隆保存；最终模式/语言在 stop 时封存。`DictationInsertionContext` 的剪贴板字段只控制结果复制/恢复，不接收剪贴板正文。问答、选区与其他服务有各自输入，不能把那些服务的全局或临时材料冒充为普通听写的段内附件。本次未找到普通听写可直接复用的截图、剪贴板正文或文件附件提交参数，没有新增附件系统，也没有宣称附件归属已经实测。
