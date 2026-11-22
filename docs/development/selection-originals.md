# 选区全文、原文历史与安全写回

本页记录从 `504747c8` 开始的开发实现与可复现证据，不代表正式发布或真实平台验收。录音收尾的恢复 lease 修复 `def20a5e` 由协调者在集成时带入；本分支不重写该 fence，也没有改版本号或安装应用。

## 原文与请求

桌面选区快捷键经过 `coordinator/hotkey_loops.rs` → `SelectionApi::begin_polish`。Windows 选区语音和 QA 中的编辑/翻译经过 `SelectionVoiceApi::begin` / `edit_preview`。以下源码路径相对 `openless-all/app`。

- `src-tauri/src/selection.rs` 的 `SelectionContext` 保存完整选中文字，保留首尾空白和中段；`SelectionInsertionTarget` 独立保存原窗口、原控件、原范围和取消资格。两者不是“当前前台应用”的别名。捕获不再生成 4000 字首尾摘要。
- `crates/openless-core/src/selection_service.rs` 在捕获后、解析服务凭据和发送模型之前，先以既有 `sessionId` 写入 `history.json` 的 `rawTranscript`，再调用 `SelectionRuntimeAdapter::protect_original`。选区语音沿同一历史构造器和同一宿主复制能力处理。没有第二个原文库。
- 原文写入失败会停止；复制失败也会停止，已经写入的原文仍可在历史取回。模型失败、HTTP 拒绝、取消或未完成输出不会删除原文。完整结果在落字前进入同条历史的 `finalText`；确认时还检查历史中原文仍然可用。终态沿 `upsert_with_retention` 更新同一个记录，不新增成功副本。
- `prompts::escape_xml_envelope` 只转义信封边界，保持完整正文；`selection_user_prompt` 和 EditPlan 的 `draft` 使用它。原有 `sanitize_for_xml_envelope` 的 16000 字预算仍用于普通听写、QA 上下文、光标上下文和指令材料。没有将这些路径一并改成无限输入。
- `DictationContext::effective_polish_prompts` 是选区生产请求的合成入口。传统渠道、原生 Gemini 和 Omni 均接收完整输入。选区的 OpenAI 兼容流必须完整结束；`length`、截断流、失败事件或无完整翻译区段会失败，不用部分输出替换，也不裁剪输入后重试。流式展示不等于有资格替换原选区。

接口契约见 `contract/backend-2.0.json` 的 `selectionOriginalProtection`。`protect_original` 返回可选的宿主剪贴板修订号；选区语音的确认票据新增可选 `originalClipboardRevision`，继续归属原 `sessionId`，不会重新捕获“当前目标”。

## 剪贴板

`src-tauri/src/insertion.rs` 沿现有 clipboard restore transaction 工作，加入 macOS `NSPasteboard.changeCount` / Windows clipboard sequence 保护。仅比较正文无法识别用户重新复制了相同内容，因此所有权同时核对修订号；相邻粘贴只在同一个仍有效的事务链上继承原剪贴板。

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
