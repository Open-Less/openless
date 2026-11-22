# 录音与长选区：原位置写回交接

后继代码基线固定为 `436f9a892a7a313e2c321eb64a452e374a1544a3`，分支 `appergb/recording-overlap`。该提交已组合录音/后台任务接口及风格祖先 `30867f75` → `3330347d`，**只作为开发接口基线，目标写回未通过整体验收，不是可发布功能基线**。本交接只更新文档，没有再次修改 selection、插入、录音或胶囊实现；原位置/选区与通用写回安全由长选区负责人统一收敛。

## 必须纠正的边界

新采集代次本身不是原输入目标失效的证据。第一段在第二段录音期间必须仍能写回原来有效的位置；成功用例必须断言实际插入成功、位置和正文正确，只有历史记录不算成功。

`436f9a89` 的两项保守策略尚不满足这个目标：

- `selection.rs:399` 的全局采集代次以及 `selection.rs:499` 的范围检查，会因为第二段开始而统一拒绝第一段。需要统一目标机制判断原位置是否真的失效，不能将“开始第二段”直接当作拒写条件。
- Windows `host_document/windows.rs:91` 只接受能用 EM_GETSEL 读取范围的 Edit/RichEdit，其他控件没有范围即拒写。这不能成为既有 Windows 浏览器/自定义控件普通单段听写的回归；需要复用可信原目标/原生范围能力，不能用全面拒写代替适配。

单看控件身份相同或数值范围相同也不足以证明原位置仍有效。确实移到另一个位置、窗口/输入框失效或原位置不能可靠核验/复原时，允许安全拒写并保留结果，绝不能盲投当前光标。程序自身已经确认成功的前段写入造成的位置推进，则不能误判成用户移动，让后段倒插或丢输出。

## 调用位置与职责

以下路径相对 `openless-all/app`，行号对应上述精确基线。

| 入口 / 阶段 | 位置 | 后继需要保持的关系 |
| --- | --- | --- |
| 捕获原输入目标 | Core `crates/openless-core/src/api.rs:8152` 的 reserve，`:8177` 调用 `TextInserter::capture_target()`；宿主 `src-tauri/src/core_adapters.rs:4231` | 在上下文/凭据等待之前取得原目标和配套上下文读取器，两者不能分属不同应用。 |
| 原目标与范围身份 | `src-tauri/src/selection.rs:66` 的 `SelectionInsertionTarget`、`:396` 的 capture、`:497` 的 current、`:533` 的 selection validation | 在这一套目标系统内收敛可信原位置；不要增加第二套目标登记或 CRDT。上述代次/范围拒写策略是待修正实现，不是后继必须遵守的接口。 |
| 已有原生目标能力 | `src-tauri/src/host_document/mod.rs:72/80/115` 的 `NativeVoiceEditTarget`、capture/apply；macOS `host_document/macos.rs:588` 的 `KeyboardDelivery` | 复用适用的目标/范围核验及已确认交付能力，核对其焦点、原文读取、隐私和线程约束，不能无条件把完整文档读取接到普通听写。 |
| 宿主插入准备 | `src-tauri/src/core_adapters.rs:4230` 延后准备、`:4280` 核验；Core `api.rs:9514` 等待原段 preparation | 后台仍使用原段捕获的 inserter，不能在 begin 时重新捕获“当前目标”替代原目标。 |
| 实际写入 | `core_adapters.rs:4790` 原目标核验、`:4872` final、`:5042/:5077/:5601` 实际派发前检查；Core `api.rs:1984/2197/2240` 平台交付边界 | 安全检查必须覆盖真正写入路径，不只停留在开始准备时；处理已派发/部分交付时不得谎报“确认未插入”。 |
| 逐段收尾和历史 | Core `api.rs:9010` finish insertion，`:9020/:9022` 保存写入错误，`:10097` 发布失败状态 | 原文、结果、历史 ID 和取消保持原 sessionId 归属；成功或失败都不改写第二段的采集状态。 |

## 保留的录音、任务和胶囊接口

- `BackendSnapshot.dictation.sessionId` 仍代表采集拥有者。Core `api.rs:8792` 只在 engine 的设备 stop 确认后腾出位置；不能为了原位置处理重新长期占住麦克风。
- `processingDictations` 保留提交顺序、原 sessionId 和后台阶段。`SubmittedDictation` 在 `api.rs:1808` 持有最终 `Arc<DictationContext>`，`submitted_dictation_context(id)` 在 `:5589` 提供只读快照。
- `submit_dictation_session(&Arc<Self>, id, options)` 在 `api.rs:8568` 拥有完整后台收尾，设备释放后可返回；旧 stop 仍可等待结果。`wait_for_dictation_submission(id)` 在 `:5596`，宿主需同时处理最终失败/取消。
- `api.rs:8814` 的后处理队列串行完成最终识别、润色和交付，第二段采集可同时运行，不要求并发运行不安全模型。
- `cancel_dictation(Some(id))` 在 `api.rs:9744` 定向操作采集段或后台段；None 只针对采集。热键入口在 `coordinator/dictation_core.rs:47`，桌面按钮入口在 `commands/dictation.rs:67/87`；不得改回全局取消。
- `DictationJobsChanged`、逐段 `DictationStateChanged` / `DictationCompleted` 不变。`capsule_snapshot.rs:150` 防止旧后台事件抢录音显示身份，`tauri_events.rs:219` 更新后台列表；前端上球体 `Capsule.tsx:928` 取消明确后台 id，下方仍是当前录音。
- 最终模式/语言与既有上下文仍按段冻结，正常风格入口仍是 `style_packs::dictation_style_prompt(style_pack)`。修目标时不要重新读取当前全局模式、风格、语言或素材。

## 写回结果契约

继续复用 `crates/openless-core/src/ports.rs:931` 的 `InsertOutcome` 和现有 `BackendError`，不要让第二套返回状态绕开历史和语义事件。

| 结果 | 必须代表的事实 | 下游处理 |
| --- | --- | --- |
| `Inserted` | 宿主确认正文已写回原有效位置 | 可供同目标的后段核验程序自身已确认的推进；不能把未确认的键盘派发当作这种证据。 |
| `PasteSent` | 已派发粘贴，尚未确认最终交付 | 保持未确认语义，不按“确认未插入”自动重试或推进后段位置。 |
| `CopiedFallback` | 已复制结果，未把本段自动写入目标 | 不显示成插入成功；保持原段历史和结果。 |
| `InvalidState` + 既有 `target_unavailable()` 标记 | 写入前已确认拒绝，本次没有派发写入 | 可用 `is_target_unavailable()` 判断；当前历史 `insertTargetChanged`、终态 `input_target_changed` 对应明确未自动输入。只有控件/范围/原文证据失效才走此分支，不能仅因后继采集开始就拒写。 |
| `OutcomeUnknown` | 已派发或可能部分写入，不能确认结果 | 当前历史 `insertOutcomeUnknown`；不允许降格为确认未插入，也不能自动重试或假定后段范围已经推进。 |

现有 `api.rs:9022` 还把所有 InvalidState 写入错误记成 `insertTargetChanged`，后继若增加其他 InvalidState 来源，应按明确的目标标记收窄分类，不能把其他原因也宣称为原目标失效。

## 必须完成的集成验收

| 场景 | 必须断言 |
| --- | --- |
| 同框光标未移动，第一段延迟期间完整采集第二段 | 第一段实际写回原有效位置；第二段首、中、尾音频完整，麦克风仍只有一个 owner，不能用“第一段保留历史”替代插入成功。 |
| 第一段成功造成已确认的位置推进后再完成第二段 | 第二段沿同一目标正确续接，顺序和正文正确；不能倒插，也不能因为旧数值位置变化拒绝并丢输出。 |
| 同框光标确实移动或选区变化 | 原位置仍可可靠定向则写原位置，否则明确拒写并保留该段结果/历史；不能写新光标。 |
| 不同应用、原窗口/控件失效、无法可靠核验原位置 | 不抢焦点、不投新目标，结果仍归属原段；拒写和不确定状态明确区分。 |
| 第一段失败/取消，第二段仍在采集 | 第二段不置 idle，不停止设备、不取消、不污染音频；后段仍可在自己的有效目标交付。 |
| Windows 浏览器/自定义控件普通单段听写 | 保留既有真实可用路径；不能统一拒写。对不支持的具体能力如实标注。 |
| 已派发但交付不确定 | 无自动重试、无假定成功推进后段；保持 `OutcomeUnknown` / `PasteSent` 对应真实证据。 |

已有测试的用途和不足：

- `crates/openless-core/src/recording_overlap_tests.rs` 验证六种双段采集/后台交错、音频首尾、历史和生产请求序列化，但没有真实目标输入控件，所以不能单独证明第一段插入成功。
- `tests/dictation_target_capture.rs:369` 是四种受控位置夹具，证明 `436f9a89` 会安全拒绝旧段并保留历史；其中“仅因后段开始而拒绝”的期望必须随统一目标方案更新，不能当作完整验收通过证据。
- `tests/dictation_target_capture.rs:444` 覆盖所选风格包、Raw 零请求/取消翻译等真实请求序列化，但其 `FocusSession` 直接记录交付，不是实际平台原位置写回。
- `src-tauri/src/selection/target_position.rs` 只验证数值范围和代次的旧判定，没有证明平台能够可靠核验/复原原位置。
- 核心采集、事件和胶囊已有通过的证据可复用，针对实际改动补目标写回和必要双段集成回归。独立 UI 测试端口保持 5511/5512，不用用户的 5435。

普通听写没有截图、剪贴板正文、文件附件提交入口的复核结论保持不变。现有应用/光标上下文、热词、纠错规则及开始时捕获的历史轮次按段冻结；剪贴板字段是输出复制/恢复设置，不是输入正文。没有附件归属实测，也不为此新增附件系统。真实模型、旧版同步及未实测平台仍由对应验收负责。
