# 选区全文、原文历史与安全写回

本页记录从 `504747c8` 开始的开发实现与可复现证据，不代表正式发布或真实平台验收。长选区实现提交为 `25045ab2`；后续在本分支保留祖先合入 Shift 最终提交 `d701af09`，同时带入录音收尾的恢复 lease 修复 `def20a5e` 与独立场景前缀 `51d183e1`（含 `95c6d204`）。本分支不重写该 fence，也没有改版本号或安装应用。

## 原文与请求

桌面选区快捷键经过 `coordinator/hotkey_loops.rs` → `SelectionApi::begin_polish`。Windows 选区语音和 QA 中的编辑/翻译经过 `SelectionVoiceApi::begin` / `edit_preview`。以下源码路径相对 `openless-all/app`。

- `src-tauri/src/selection.rs` 的 `SelectionContext` 保存完整选中文字，保留首尾空白和中段；`SelectionInsertionTarget` 独立保存原窗口、原控件、原范围和取消资格。两者不是“当前前台应用”的别名。捕获不再生成 4000 字首尾摘要。
- `crates/openless-core/src/selection_service.rs` 在捕获后、解析服务凭据和发送模型之前，先以既有 `sessionId` 写入 `history.json` 的 `rawTranscript`，再调用 `SelectionRuntimeAdapter::protect_original`。选区语音沿同一历史构造器和同一宿主复制能力处理。没有第二个原文库。
- 原文写入失败会停止；复制失败也会停止，已经写入的原文仍可在历史取回。模型失败、HTTP 拒绝、取消或未完成输出不会删除原文。完整结果在落字前进入同条历史的 `finalText`；确认时还检查历史中原文仍然可用。终态沿 `upsert_with_retention` 更新同一个记录，不新增成功副本。
- `prompts::escape_xml_envelope` 只转义信封边界，保持完整正文；`selection_user_prompt` 和 EditPlan 的 `draft` 使用它。原有 `sanitize_for_xml_envelope` 的 16000 字预算仍用于普通听写、QA 上下文、光标上下文和指令材料。没有将这些路径一并改成无限输入。
- `DictationContext::effective_polish_prompts` 是选区生产请求的合成入口。传统渠道、原生 Gemini 和 Omni 均接收完整输入。选区的 OpenAI 兼容流必须完整结束；`length`、截断流、失败事件或无完整翻译区段会失败，不用部分输出替换，也不裁剪输入后重试。流式展示不等于有资格替换原选区。

接口契约见 `contract/backend-2.0.json` 的 `selectionOriginalProtection`。`protect_original` 返回可选的宿主剪贴板修订号；选区语音的确认票据新增可选 `originalClipboardRevision`，继续归属原 `sessionId`，不会重新捕获“当前目标”。

## 剪贴板

`src-tauri/src/insertion.rs` 沿现有 clipboard restore transaction 工作，加入 macOS `NSPasteboard.changeCount` / Windows clipboard sequence 保护。仅比较正文无法识别用户重新复制了相同内容，因此所有权同时核对修订号；仅在当前事务仍持有剪贴板时恢复原文。

桌面宿主的事务实现位于 `src-tauri/src/insertion/clipboard_delivery.rs`。普通听写、选区原文保护、显式复制、结果兜底和选区捕获的临时借用共用交付锁。一次自动粘贴在锁内保存原剪贴板、写入结果、复核原目标与剪贴板所有权、派发、等待既有 750ms 消费窗口，并按需恢复；关闭恢复也必须等消费窗口结束后才允许本进程另一次写入。恢复不再作为脱离交付锁的迟到线程执行，也不需要重叠事务的 pending 链。模型处理和麦克风采集不持有这把锁，显式复制 IPC 在 blocking worker 等待；热键边沿的模拟复制若遇忙则跳过该捕获路径，不阻塞热键线程。

`insert_selection_result` 接收同一会话保存的完整 `source_text`，等待锁后及派发前重新核对取消资格、原控件、原范围和完整原文。等待期间目标变化或取消时不借用剪贴板，结果仍在历史；已借用但确认没有派发时才允许结果兜底。原文保护在锁内取得复制修订号，条件兜底的修订号核对与写入也在同一锁内。自动派发先返回 `PasteSent`，macOS 只有既有 `KeyboardDelivery` 确认后才升为 `Inserted`；Windows 部分派发错误仍保留不确定结果，不再复制或重试。

这把锁只能协调本进程。外部应用可以在最后一次核验之后改变焦点或剪贴板，系统粘贴快捷键也没有通用的消费完成回执；超过既有 750ms 窗口的消费仍不能保证。修订号会拒绝可观察到的后续复制（包括相同文本及图片），并阻止迟到恢复或兜底覆盖它；OS 的“核验后、派发/恢复前”小窗口以及模拟复制期间无法区分目标复制与用户复制的情况不能原子封锁。恢复仍沿既有纯文本能力，不承诺保存借用前的图片或富文本全部格式。

受控宿主回归运行 `cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml --test clipboard_delivery --test selection_clipboard`。夹具调用生产事务，控制剪贴板、派发和延迟消费，覆盖原文保护/显式复制插入交付窗口、后续复制优先、完整原文恢复、等待时目标变化/取消、错误释放及不确定派发；它不代表物理 OS 剪贴板或焦点竞态验收。

选区处理在模型开始前复制完整原文一次。实际粘贴暂借剪贴板，选区专用路径强制恢复，并在既有消费延迟结束后才返回。成功交付不会再次复制处理结果；期间用户主动复制的新内容优先，不会被迟到的恢复或兜底覆盖。若模型返回前用户已另行复制，选区自动粘贴也会停止，完整结果留在历史；这同时避免用纯文本恢复去破坏用户新复制的图片或富文本格式。

目标变化、原选区/范围变化等**确认未派发写入**的情况，只有原剪贴板事务仍持有剪贴板时才复制完整结果作为兜底。用户已经复制别的内容时保留其剪贴板，完整结果和原文仍在历史。已派发但未确认的交付保持 `PasteSent`，部分 Windows SendInput 也不再被当成“确认没有插入”而再次落字或复制。`OutcomeUnknown` 不允许重试确认。

macOS 使用既有 `KeyboardDelivery` 核对交付后的原控件范围；Windows 粘贴继续如实报告 `PasteSent`，没有仅凭 Ctrl+V 派发就声称插入成功。确认插入后，现有不激活窗口的反馈显示“如果有掉词，可前往 App 的历史记录获取原文。”，提供八种界面语言。

## 原位置与连续录音

`selection.rs` 复用同一目标模型。macOS 保存 AX 控件身份和窗口 ID；Windows 保存 HWND、进程/线程及 UIA runtime ID。选区写回还比较原范围和完整原文，不能因另一输入框或另一个位置恰好选中同样文字而放行。最终复核只读原生选区，不再用 Cmd+C / Ctrl+C 改动异步阶段的剪贴板。

普通听写不再因全局采集代次变化就废弃旧目标。同一仍有效的控件共享原范围；只有前段交付得到确认，且实际新范围等于预期推进，才允许后段续接。用户移动光标、选区内容变化、失效窗口或无法核实范围仍会拒写。程序没有抢回后台应用的普通听写焦点；用户明确确认预览时可沿既有机制返回原应用，随后仍需核对原控件和范围。

Windows 原生 Edit/RichEdit 继续使用 `EM_GETSEL`；浏览器/自定义控件可以用 UIA TextPattern 的端点移动取得位置，不读取完整文档。既无可用原生范围也无 UIA 文本范围的具体控件仍拒绝自动写回，不能把 PID 或窗口相同当作输入框/位置证明。普通听写的上下文预算没有因此扩大。

## 历史界面与平台范围

`src/features/history/HistoryDetail.tsx` 原有“原文”视图及复制操作直接使用完整 `rawTranscript`；`HistoryList.tsx` / `History.tsx` 增加常驻复选框和“复制所选原文”。单条与多条使用同一复制操作，多条按当前列表顺序以两个换行分隔，每条内部正文和边界空白不变。工具栏保持固定位置，八种语言均有文案。

Linux 的既有 fcitx5 直接目标路径在处理前使用现有复制能力，移除按旧摘要形状误拒绝正文的判断；本机未运行 Linux。iOS 的 `SuppliedTextRuntime` 是只接收 App 显式文本并返回预览的既有入口，不能捕获或覆盖外部选区；保持其“不自动读写剪贴板”的承诺，仍保存完整输入到同一历史，不把它计为桌面系统选区的验收。

## 要求 → 入口 → 源码 → 证据与限制

| 要求 | 实际入口与源码 | 验证 / 限制 |
| --- | --- | --- |
| 普通、超过 4000、超过 16000，首尾空白和独特中段完整 | `SelectionApi::begin_polish` → `selection_service.rs` → 生产 `SharedCloudTextPolisher` | `tests/selection_contract.rs::normal_selection_entry_serializes_full_input_and_rejects_incomplete_streams` 捕获真实 HTTP 请求 JSON，比较完整 user 消息；另覆盖 48000 字输入及捕获层保真。服务是本地可控夹具，非真实模型。 |
| 选区翻译也完整传入模型 | `SelectionVoiceApi::edit_preview` → `selection_voice_service.rs` | 同一 HTTP 集成用例包含超过 16000 字翻译入口；`selection_voice_contract` 另覆盖长草稿编辑/翻译及字段上下文。 |
| 在服务失败之前保存和复制，失败可找回 | `begin_polish` / `begin` / `edit_preview` → 既有 `HistoryStore` 和 `protect_original` | 原文保护夹具在复制回调中读取真实历史文件并断言原文已持久化；磁盘路径故障、复制失败、HTTP 400、`length`、流中断均不调用替换。没有只测纯 prompt helper。 |
| 成功后原剪贴板，明确未写回才结果兜底；不覆盖新复制 | `selection::insert_selection_result` / `selection_result_fallback` → `insertion.rs` | 宿主 clipboard transaction 单测覆盖事务链和修订号；生产路径强制恢复原文。真实系统剪贴板成功/失败及用户复制竞态尚未做真机端到端验收，不能据单测声称已经通过。 |
| 同 app 换输入框/位置不误写；新录音本身不废弃原目标 | `capture_dictation_insertion_target` / `selection_insertion_target_is_current` / `confirm_target_delivery` | `tests/dictation_target_capture.rs` 在正常 Core start/stop 下覆盖移动拒写，以及第一段延迟期间第二段采集、第一段实际交付和第二段正确续接的位置/正文断言；是受控输入目标，不是真实 AX/UIA 控件。 |
| 不完整流不替换，不确定交付不重试 | 生产 `selection_streaming`、`TextEventStream`；`SelectionService::settle_confirm_failure`；既有 `InsertOutcome` | 请求集成夹具及协议/润色回归；`PasteSent` 保持未确认，历史保留完整原文。Mac/Windows 的真实焦点变化、系统交付回执尚未实测。 |
| 详情、单条及批量取回原文，窄窗口无横向溢出 | 历史页面正常导航 → 原文视图/复制、列表复选框 | `e2e/selection-original-history.spec.ts` 使用正常 React/宿主 IPC 入口，核对 18000+ 字原文、首尾空白、单条及批量复制正文和窄窗口布局；Chromium 通过。 |

定向命令从 `openless-all/app` 运行：

```sh
cargo test --locked -p openless-core --test selection_contract --test selection_voice_contract --test dictation_target_capture
cargo test --locked -p openless-core --lib selection_voice
cargo test --locked -p openless-core --lib polish::tests
cargo test --locked -p openless-core --lib llm_protocol::tests
cargo test --locked -p openless-core --lib llm_gemini::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib insertion::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib selection::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib selection_target
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib core_adapters::tests::
OPENLESS_E2E_PREVIEW_PORT=5571 OPENLESS_E2E_FIXTURE_PORT=5572 npx playwright test e2e/selection-original-history.spec.ts --project=chromium
```

界面已经 `npm run build`，macOS 宿主已编译并运行上述不需要真实编辑器的单元测试。未做发布构建、签名、安装或部署；未触碰真实用户数据。真实 macOS/Windows 编辑器、Windows 浏览器、Linux/fcitx5、iOS、系统剪贴板及物理麦克风需要后续平台验收，不能用这些夹具代替。

## 与 Shift、场景及恢复 lease 的组合验收

组合由 `25045ab2` 合入 `d701af098a500809d2c44aeac765d5cfe0fd2756`，没有文本冲突。共享的 `api.rs`、`cloud_providers.rs`、目标集成测试和热键桥接均自动合并；除新增一条联合回归及本节记录，没有手工修改生产逻辑、风格或手势 API。

协调者已代表场景/Shift owner 确认请求合成边界：普通前台听写在最终请求最前方合成一次冻结的场景，所选风格仍保留原身份；`selection_input` 显式排除场景前缀，继续通过 `effective_polish_prompts` 传递完整选区。选区分支没有在调用方再加一次场景，原文不随场景合成而 trim 或截断。

| 新交叉风险 | 实际入口 / 源码 | 本次证据与复用范围 |
| --- | --- | --- |
| 场景独立、风格之前且一次，调用者 `front_app` / 明确风格守卫不丢 | `tests/dictation_target_capture.rs::routed_dictation_sends_the_frozen_pack_and_delivers_to_the_original_app` → 正常 start/stop → 生产 SharedCloudTextPolisher → 本机 HTTP | 合并后原有六项目标集成测试通过；该生产请求测试继续断言前缀唯一、场景在风格前、显式应用上下文和选择守卫。`dictation_context.rs::with_input_scene_prefix` 仍显式排除 `selection_input`；没有更改 owner 的合成接口。 |
| canonical Raw + 表达 profile + 翻译起录后取消零模型，自定义 Raw 保持模型行为 | 同一 `routed_dictation...` 生产请求测试 | 定向关闭与 stop 时关闭两条 canonical Raw 路径请求数为零；普通 Raw 也为零；自定义 Raw 恰有一次请求。普通润色在全局 profile 被清空后取消翻译，仍发送一次开始时冻结的表达指令；翻译不带该指令。复用合入的 d701af09 断言，无另写表达/风格实现。 |
| 同次流程证明实际交付与音频完整，而非拼接两套独立证据 | 新增 `tests/dictation_target_capture.rs::overlapping_pipeline_preserves_audio_modes_and_confirmed_target_progress` | 一条测试的两个受控子场景都使用真实 DictationEngineRouter / PipelineDictationEngine、受控 Recorder / Transcriber / Polisher 和既有 Focus 目标适配器。第一段转写延迟期间启动第二段；第一段最终返回 `Inserted`，`delivered` 含 `polished-first` 且位置为 4，同时第二段仍为 Recording、唯一麦克风 owner 为 2。第二段最终首中尾 PCM 为 `[2,1,2,2,2,3]`；第一段为 `[1,1,1,2,1,3]`。第一段最终普通模式、第二段最终翻译模式，调用和历史均按原 sessionId 归属。 |
| 已确认写入后续接、用户移动拒写 | 同一新联合测试 → 既有 FocusInserter / FocusSession → 生产 `selection/target_position.rs` 判定 | 未移动时 `delivered` 顺序为 `polished-first`、`translated-second`，第二个位置为 `4 + utf16_len(polished-first)`；另一子场景在第一段交付后将用户位置改为 88，第二段明确拒写，`delivered` 仍仅第一段且位置 88 不被改动，同时第二段声帧、结果与翻译历史完整保留。不是物理麦克风或 AX/UIA 真机输入。 |
| 选区首尾空白、中段和超过 16000 字不因场景合成丢失 | `tests/selection_contract.rs` → 正常 SelectionApi / SelectionVoiceApi → 生产 HTTP 序列化 | 合并后 21 项选区测试通过，包含完整 user 消息、独特中段、原文历史、翻译入口、HTTP 拒绝及不完整流。其余原文存储/剪贴板/目标生产文件相对 `25045ab2` 无组合差异，因此复用首轮事务、未派发兜底、新复制优先及不确定交付的证据和限制，不冒称真实系统剪贴板已验收。 |
| 恢复 admission 继续覆盖最终历史落盘 | `api.rs` 合入的 `_finalization_resources` → `api::tests::submitted_dictation_restore_fence_covers_final_history` | 本次定向重跑通过：两个显示槽已经空时，恢复仍受资源 lease 阻挡，直到最终历史完成。没有重跑 211 项旧版同步矩阵或修改同步协议。 |

本次执行并通过：

```sh
cargo test --locked -p openless-core --test dictation_target_capture --test selection_contract
cargo test --locked -p openless-core --test dictation_target_capture overlapping_pipeline_preserves_audio_modes_and_confirmed_target_progress
cargo test --locked -p openless-core --lib submitted_dictation_restore_fence_covers_final_history
cargo check --locked --manifest-path src-tauri/Cargo.toml --lib
npx tsc --noEmit
```

第一条为合并后的既有 6 项目标集成 + 21 项选区回归；第二条为新增联合测试的两个子场景；第三条为单项恢复 fence 回归。macOS 编译保留现有告警，未开展 Windows 编译、真实系统剪贴板/焦点竞态、真实模型或物理硬件验收，也未重跑所有旧套件。组合达到开发集成检查标准后停止，发布资格仍由后续平台验收决定。

## Beta.2 准备基线与 Beta 搜索的有限整合

在 `b3f74d41` 之后，先保留祖先合入 `beta@670a44a03fc1415629553ccc3de9a02bf8205919`，再合入搜索主 tip `740f59e4ecac8d9e43dc641d1422675acfd3be53`（含 `2870ab58`、`c216c479`）。Beta.2 的 `2.0.3-Beta.2+build.noask.20261123` 和前后端 Ask 关闭标记原样继承，没有回退版本，也没有修改原 beta 分支、标签或安装应用。

Beta 基线自动合并；搜索只有 `docs/index.md` 发生冲突，处理为同时保留 v1 同步兼容文档和新增的联网搜索文档入口。搜索三条提交相对其 `51b65224` 基线只涉及九个搜索源码、设置和文档文件；没有合入 `d266ea89`、`c27d27f7`、`8d93c065` 的 Ask 兼容工作，没有引入 PersonalAssistant 产品，也没有合入付费 live CI tooling 或更改 `ci.yml`。

本轮直接交叉检查：

- 搜索 `web_search` 单测：9 项通过，3 项 live 默认忽略；旧 QA 生产公共入口的搜索配置与 Off 回归：2 项通过，2 项 live 默认忽略。没有调用付费 DeepSeek，也没有重试官方 key；已知官方 401 仍阻塞真实 DeepSeek 成功验收。
- 长选区实际 HTTP 序列化回归 `normal_selection_entry_serializes_full_input_and_rejects_incomplete_streams` 通过：首尾空白、超过 16000 字正文、翻译、历史及不完整响应拒写仍保留。
- 搜索服务配置来回切换、保存重开、Beta.2 Ask 关闭版本判断、TypeScript 和 macOS 宿主编译通过。搜索设置保留原 ID 和各服务配置、`providers` 溯源；`web_search.config` 仍在原本机 secret，没有扩大到 Portable/preferences 或云凭据白名单。
- 原始搜索 UI codec 的跨旧版扩展丢失边界脚本通过，仍明确旧版编辑会删除扩展配置；没有重复 211 项旧版同步矩阵。已通过的录音音频/目标联合回归直接复用，没有再次运行。
- 为匹配共享 Core 的严格检查，仅把联合测试读取 PCM 时的互斥锁在异步 shutdown 前显式释放；不改变断言、音频、模式或目标生产逻辑。

随后只向自己的开发分支推送并创建指向 beta 的 Draft PR，使用仓库正常 PR CI 按真实影响决定门禁，不添加跳过共享 Core 的标签，不手动重复触发 workflow 或发布构建。CI 链接、终态与具体失败由本轮交接和 PR 记录承接；即使 CI 通过，也不代表 Windows 真机、系统剪贴板/焦点、物理音频、安装 GUI 或真实 DeepSeek 已验收。此组合不可直接合并 beta 或公开发布，仍由主协调者最终验收。

### 首次正常 PR CI 的具体发现

组合 `c3c7e298` 推送到自己的开发分支后创建 [Draft PR #1487](https://github.com/Open-Less/openless/pull/1487)，触发唯一一次正常 [CI 45064779185](https://github.com/Open-Less/openless/actions/runs/45064779185)，未加标签、未手动 dispatch/rerun、未触发发布流水线。该记录不能写成全部通过：

- Windows Rust 1.99 MSRV 已通过后端检查和测试编译；原生胶囊的 macOS/Windows 门禁通过。它们是 CI 编译/测试证据，不是用户输入框或硬件验收。
- macOS 前端门禁在 `capsule-ready-replay.test.mjs` 失败：生产效果已新增 `dictation:jobs` 订阅及两个状态 setter，而旧夹具仍期待四个主窗口订阅且没有这些 setter。已同步夹具到五个订阅，保留原有有界等待和清理断言，新增“迟到快照不覆盖实时后台任务”断言；本地该定向脚本通过。
- Windows stable 在 `windows-ui-config.test.mjs` 失败：脚本只查找旧的 `if (insertFallback)` 字面量，录音组已经把它扩为条件表达式，导致脚本找不到位置而误报 hook 排序。更新为匹配该条件的开头，仍要求可见性 hook 在返回分支之前、后面不再调用 hook；本地通过，没有改 UI 行为。
- Android 编译揭示三个条件编译遗漏：移动端没有桌面位置核验函数，也没有桌面原生胶囊字段。桌面 AX/UIA 检查和原生后台球体更新已限于桌面编译，移动端继续使用原 IME/无障碍插入策略，不新增返回“目标可信”的假能力。修复后的 macOS 编译通过；Android 必须由后续真实目标编译确认。

其他首轮门禁仍需以该次运行最终记录为准。上述修复先形成独立本地提交；由于任务明确只触发一次正常 CI，是否推送修复而自动产生后继 PR CI，先由协调者明确收敛，不用手动重跑或标签绕过失败。

### 后继候选的因果修复记录

协调者随后明确：不同提交因具体失败证据产生的最小修复，可以正常 push，由 PR synchronize 自动运行新候选 CI；禁止的是没有新代码/失败证据时对同一 SHA 反复手动重跑。`fe7fec21` 已按此授权推送，对应 [CI 45065448470](https://github.com/Open-Less/openless/actions/runs/45065448470)。该候选的 Android 门禁、两端 MSRV、两端原生胶囊及 Harmony 已通过，Windows 首次原生类型和测试编译已有 CI 证据，但本轮仍暴露以下失败，不能称为绿灯：

| 具体失败 | 原因与最小修复 | 定向证据 / 保留的约束 |
| --- | --- | --- |
| 两个 Core busy/取消 apply-ticket 测试报 `selectionOriginalCopyFailed` | 旧测试使用没有原文保护能力的 backend。只为这两个测试显式配置既有 FixtureSelectionRuntime / Polisher，默认 unsupported 夹具保持不变。 | 两项本地通过；没有跳过生产原文保护，取消票据仍必须等原生 completion 才释放恢复 lease。 |
| Gemini 流式翻译正例被判不完整 | 模型夹具没有 Gemini 的 `finishReason=STOP`；补正确的终止事件，不放宽 parser。 | `gemini_translation_streams_target_before_completion` 本地通过；STOP/截断失败语义保留。 |
| 旧延迟 completion 测试等不到第二段 | 夹具引擎沿 legacy 默认返回“不确认设备停止”，却要求在旧完成回调未结束时重用采集。仅该用例使用显式确认 stop 的无设备引擎。 | 该单项本地通过，原来的旧回调不得重置/观察新段断言全部保留；生产资源 fence 不修改。 |
| iOS 隔离 Core 的音频用例设置 Japanese 报未知目标 | iOS 已有目标清单契约不接受未经配置的新选择。改为清空全局目标，并继续断言两段冻结的 English 请求和完整声帧。 | 只因实际失败重跑该录音回归，本地通过；没有更改 iOS 生产目标语言策略，也没有重复已通过的目标/音频联合测试。 |
| legacy 本机自捕获数据无法恢复 | 新官方 intent 包是第五个 canonical Builtin，而旧准入只接受四个按 mode 推导的 ID。按 owner 确认一次读取现有官方目录，以 ID + base_mode 配对准入。 | 旧两个失败恢复测试与新增正反例通过：五包含 intent、选中引用和用户自改 canonical prompt 保留；同名未知 ID、错 mode、非法 ID、Imported 保留前缀拒绝且不写仓库。未更改先准入后 normalize 顺序、v1 旧四 ID imported 投影或 secret 范围；Builtin 非 canonical alias 仍不属于此 legacy 修复的承诺。 |
| 历史行外框比搜索框缩进约 35px | 新复选框占用了卡片外部布局宽度。复选框保持独立原生控件，定位在全宽行表面内部，正文预留空间。 | 原 geometry 测试通过，未放宽对齐值；单条/批量真实 checkbox 交互、首尾空白复制和滚动检查通过。 |
| 两条搜索页面测试仍查旧文案 | 功能名称已明确为“免费聚合搜索 + DeepSeek Web Search”。只更新标签与免费入口期望。 | 两项受影响页面测试通过，330/1300px 对齐、防裁切、保存失败及离页保存反馈断言均保留。 |
| Windows modifier-chord 第一个 Pressed 通道断开 | owner 确认是同测试进程的全局 ACTIVE_MONITOR 被并行 start/drop 替换。新增仅 cfg(test) 的共享生命周期锁，相关 native dispatcher 与唯一全局监听器恢复测试共用。同步测试用 blocking_lock，异步 native 测试用同一锁的 async guard。 | 受影响的 macOS harness 51 项通过；Windows 的 Pressed → 同 id 唯一 Combined → repeat 零 → Released 原断言未删除，需下一候选 Windows CI 执行。未改生产单例、手势排除规则或全仓测试并行配置。 |
| Classic 仅翻译徽章仍期待 top=88 | 录音 owner 用身体124..156、上球体106..126和徽章72..100独立核对：旧值会重叠球体。仅更新遗漏的测试期望为72并补相对几何。 | Native 单项和 TS 几何测试通过；保留24px身体预留、2px球体连接、文本 rail48、复合翻译 rail top12/height88，生产布局不改。 |

本地受影响的 Chromium 四项页面测试通过；未以它代替 WebKit、Windows 或安装 GUI。后续以新提交的正常 PR CI 为准，未手动重跑旧 SHA、未调用模型 live、未重复 211 旧版矩阵。

### 隔离 iOS Core 的最后入口检查

`f5fd1b44` 的 [CI 45067669435](https://github.com/Open-Less/openless/actions/runs/45067669435) 中，Windows stable/MSRV、macOS/MSRV、两端原生胶囊、Android、Harmony、iOS device-build 及 Chromium/WebKit 均通过；iOS `test` 在 Host 的一条旧预览断言失败。该断言仍要求历史为空，与处理前保存完整原文的新契约矛盾。现改为检查一条历史保留首尾空白的原文、完整预览及 `NotRequested`，同时保留 Cancelled 终态和没有待插入请求的检查；iOS 仍不自动读写剪贴板或写入外部选区。本机精确运行该 Host 用例通过。

为验证此修复之后声明的严格门禁，直接编译隔离 Core 的目标集成测试，复现其读不到 `src-tauri/src/selection/target_position.rs`：准备脚本原来只共享 assets/contract/src，没有给测试所引用的生产范围 helper 提供只读路径。沿原有 source-link 机制补充 `src-tauri`，不复制或改写宿主源文件；该目标随后编译通过。

本机同一 `cargo clippy -p openless-core --all-targets -- -D warnings` 入口进一步指出本批引入或暴露的静态问题，均按具体原因修复：去掉多余借用；把两个声帧用例的互斥锁限定在同步断言块中，避免显式 drop 仍被判断跨 await；为重复的夹具类型命名；断言改为借用单元素 slice；超时来源夹具按完整 HTTP 请求头读取并检查真实 EOF；偏好测试使用原值的结构体初始化；仅测试使用的 legacy prompt helper 移到测试模块之前，函数内容不变。没有添加 `allow`、`ignore` 或降低 `-D warnings`。

本机的 iOS Host 精确用例、隔离目标编译、严格 all-targets 检查及受影响的来源超时单测均通过。本机编译器为 Rust 1.100，CI 仍使用原来固定的 Rust 1.99；因此新提交继续由正常 PR CI 确认，不能把本机结果写成固定 CI 版本已通过，也不重复执行无关业务矩阵或 live 模型。
