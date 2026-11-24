# 正式版普通输入契约

输入行为的来源是已发布的 `v2.0.2-tauri`，源码 `962705d56984b4c267f450d016aa16b3a881e4b4`，不是 Beta.2（`670a44a0`）。本次恢复基于 DEV `ad276770`，不改变版本号、UI 或选区编辑。

## 实际入口

- 热键：`src-tauri/src/coordinator/dictation_core.rs::dispatch` → Core `dispatch_dictation_hotkey_edge` → `start_dictation*` / `stop_dictation*`。
- React 胶囊：`src/features/capsule/Capsule.tsx::onConfirm` → `src/api/dictation.ts::stopDictation(sessionId)` → `commands/dictation.rs::stop_dictation` → `submit_dictation_session`。原生胶囊 `native_capsule.rs::Action::Confirm` 也调用后者。取消携带明确 sessionId，只清理该段。
- 胶囊没有手动文字编辑器。`TranscriptDelta` 更新胶囊原文；这不是向外部输入框落字。外部组合文本由 `ActiveTextInsertion::push_partial` 处理，润色分块由 `ActiveTextInsertion::push` 处理，最终结果由 `finish_text_insertion` → `TauriTextInsertionSession::finish` → `insert_final` 交付。

## 正式版、恢复前 DEV 与恢复后

| 行为 | 正式版证据（固定源码行号） | 恢复前 DEV | 恢复后 |
| --- | --- | --- | --- |
| 捕获目标 | `core_adapters.rs:4122` 的 `capture_target` 保存 `Some(insertion_target)` 和 Windows IME 目标 | 只保留提示上下文；实际目标到交付时才捕获 | 恢复开始时目标；模型应用上下文仍冻结 |
| 恢复目标 | `core_adapters.rs:4664` → `selection.rs:519` | 普通输入跳过恢复与范围检查 | 恢复原目标，不使用后来添加的严格光标范围策略 |
| macOS 控件/光标 | `selection.rs:556,570` 只确认原应用 pid；没有冻结 AX 控件或光标范围 | 向交付时的当前应用输入 | 回到原应用，用它当前的控件/光标；同应用切控件/移光标可继续输入 |
| Windows | `core_adapters.rs:4746`，恢复原窗口后捕获 TSF Commit 目标 | 从交付目标直接构造 TSF 目标 | 恢复窗口、核对原窗口/控件；TSF 组合仍属于捕获的原 native context |
| 无法恢复 | `core_adapters.rs:4746` 的恢复错误分支 | 普通输入没有此检查 | macOS 仅复制；Windows 按已有非 TSF 回退开关复制或报错，避免写入无关应用 |
| 实时写入 | `core_adapters.rs:5077` 按平台返回能力，Core 按偏好判断 | 桌面关闭外部组合和润色分块 | 恢复平台能力和偏好；胶囊原文仍独立实时更新 |
| macOS 普通粘贴 | `core_adapters.rs:4941` → `insertion.rs::insert`，成功派发即报告 Inserted | AX 回执决定成功，未确认时留下结果剪贴板 | 恢复正式版派发语义，无需 AX 回执；按恢复开关恢复旧剪贴板 |

恢复保留共享 `clipboard_delivery` 应用内事务锁、原生固定 750ms 等待及恢复前的文本/版本所有权检查。在这段有界派发、等待与恢复窗口内，应用自己持锁的剪贴板写入互斥；恢复检查发现较新的用户复制时不写回。750ms 之后目标应用是否已读取结果并无保证，所有权检查与恢复写入也不是操作系统原子事务，不能保证与用户复制完全无竞态。macOS 沿正式版在成功派发后视作交付，不能据此证明实际消费，读取较慢的目标仍可能错过结果或读到恢复后的旧剪贴板。没有复制旧版异步恢复队列。Windows 部分 Unicode 派发和 TSF OutcomeUnknown 仍保留 DEV 的“不自动重试”语义，避免重复写入；不能把已派发但未知的结果假称为只复制。

## 与连续录音的最小适配

Core 仍允许设备停止后立即采集下一段，并按同一处理锁串行完成最终 ASR、润色与写入。桌面保留 `defer_preparation_until_stop`，在取得该锁后、引擎 finish 前准备原目标，恢复润色分块能力；Windows 组合也仅在这个阶段打开。因此实时 ASR 原文可以在录音期间显示在胶囊，目标组合不会在排队采集期间打开。准备读取同 sessionId 的已封存上下文，保留停止前 Shift 选择，不读取下一段的模式。正式版在录音期间打开组合的时机没有照搬，这是与当前并发采集必要的适配。

选区润色、翻译、voice-edit 继续走严格的 `reactivate_selection_insertion_target`、完整 source_text 与范围验证；普通输入使用共用激活实现的非严格 macOS 分支。选区历史、原文剪贴板保护及所有权没有放宽。

## 验证边界

本轮本机仅执行源码检查、`git diff --check` 和不编译的 `node scripts/windows-insertion-target-contract.test.mjs`。新增/调整的 Rust 夹具必须在 GitHub 云端运行；它们不代表真实桌面、麦克风、TSF 或 AX 验收。

在 `openless-all/app` 运行最小云端检查：

```sh
cargo test --locked -p openless-core --test dictation_target_capture
cargo test --locked -p openless-core --lib recording_overlap_tests
cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml --test clipboard_delivery
node scripts/windows-insertion-target-contract.test.mjs
```

使用现有 CI 的 macOS、Windows 和 Android 宿主编译检查验证 cfg 分支；不在本机生成编译产物。未运行真实用户应用输入或真实模型请求。
