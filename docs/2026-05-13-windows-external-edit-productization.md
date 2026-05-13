# Windows external-edit 产品化说明

## 目标

本次收敛把 `external-edit-lab` 接回正式 OpenLess 听写链路，形成一个仅限 Windows 首批发布的最小正式能力：

- OpenLess 在支持场景中正式插入文本后，自动 arm 一个短窗口 observer。
- 如果用户随后在同一目标控件里人工改正刚插入的术语，系统尝试抽取 deterministic 的 `old -> new` 替换对。
- 学到的替换写入本地正式 terminology memory。
- 后续正式听写优先命中该记忆。
- 任一失败都只能静默回退，不能影响本次原始插入结果。

## 正式接入点

### 1. 插入完成后如何 arm

- 接入文件：`openless-all/app/src-tauri/src/coordinator/dictation.rs`
- 触发时机：正式 dictation pipeline 完成 transcript 获取、correction rules 应用、插入之后。
- 当前只在以下条件满足时 arm：
  - Windows
  - `windows_external_edit_learning = true`
  - `InsertStatus::Inserted`
  - `focus_ready_for_paste = true`
- arm 输入最小化为：
  - `inserted_text`
  - `window_title`

### 2. observer 生命周期

- 实现文件：`openless-all/app/src-tauri/src/windows_external_edit.rs`
- 行为：
  - arm 后先等待很短的 baseline delay。
  - 读取当前 focused element 文本作为 baseline。
  - 只在短 observation window 内轮询。
  - 新 session 开始时会 cancel 上一个 observer，避免跨 session 污染。
- 读取路径：
  - Windows UI Automation `ValuePattern`
  - fallback 到 `TextPattern`
- 首批不引入常驻监听器，不增加新权限，不在 macOS 上实现 observer。

### 3. 观察成功后如何写入正式 terminology memory

- v1 不新增 `terminology-memory.json`。
- 正式 terminology memory 直接复用：
  - `%APPDATA%\OpenLess\correction-rules.json`
  - 存储实现：`CorrectionRuleStore`
- `CorrectionRuleStore::remember(pattern, replacement)` 规则：
  - 已存在同 `pattern + replacement`：直接返回；如果原先 disabled，则重新启用。
  - 已存在同 `pattern` 但不同 `replacement`：拒绝写入，避免冲突覆盖。
  - 否则插入新 rule。

### 4. 后续正式输出如何 deterministic 命中

- 不新增第二套 rewrite 逻辑。
- 继续复用正式链路中的 deterministic correction rules：
  - `coordinator/dictation.rs`
  - `apply_correction_rules(...)`
- 这意味着 external-edit 学到的规则，与用户手工维护的 correction rules，共用同一正式命中点。

## 失败回退契约

以下任一失败都不得影响本次原始插入：

- baseline capture 失败
- focused element 无法读取
- inserted text 在 baseline 中不唯一
- diff 无法确定归因到刚插入 span
- deterministic inference 失败
- 写入 `correction-rules.json` 失败

对应日志统一走 `[extedit] ...`，但不弹窗、不阻断、不改写本次插入结果。

## 支持矩阵

### 首批正式支持边界

| 目标 | 状态 | 证据状态 | 备注 |
| --- | --- | --- | --- |
| Windows Notepad | 已知支持 | 仓库内可复跑正式插入 smoke + observer evidence | 首批正式支持 |
| Microsoft Edge textarea | 已知支持 | 当前 worktree 已补齐正式插入 smoke + observer evidence | 首批正式支持 |
| WeChat 输入框 | 已知支持 | 历史真实外部编辑验证已确认；当前 worktree 未补新正式证据包 | 首批正式支持 |
| Zed | 不支持 | 保留不支持边界 | 不进入首批支持矩阵 |
| Windows Terminal | 不支持 | 保留不支持边界 | 不进入首批支持矩阵 |
| VS Code | 未验证 | 当前机器未验证 | 不宣称支持 |

### 说明

- 首批正式支持矩阵只包含：
  - Windows Notepad
  - Microsoft Edge textarea
  - WeChat 输入框
- 但当前仓库内已经补齐、可直接复跑的正式 observer 证据资产，覆盖：
  - Notepad
  - Edge textarea
- WeChat 仍需按 runbook 重新沉淀当前 worktree 的 stdout / JSON / log / rule-file 证据，之后才算“证据齐全”。

## 最小用户可见入口

- Settings -> Advanced
  - 新增 `Windows external-edit auto learning` 总开关
- Vocab -> Correction rules
  - 继续承担查看、禁用、删除、验证已学规则

v1 不新增独立设置页，不做导入导出，不做云同步。

## 日志字段

关键日志前缀：`[extedit]`

预期关键行：

- `armed observer`
- `learned rule ...`
- `persisted rule id=...`
- `observation skipped: ...`
- `observation failed: ...`
- `observation cancelled`

日志路径：

- `%LOCALAPPDATA%\OpenLess\Logs\openless.log`

## 验证分层

### 1. pure

- 代码位置：`openless-all/app/src-tauri/src/windows_external_edit.rs`
- 覆盖：
  - literal replacement
  - numeric generalization
  - reject diff outside inserted span
  - reject ambiguous inserted span

建议命令：

```powershell
cargo test --manifest-path openless-all/app/src-tauri/Cargo.toml --lib --no-run
```

说明：

- 当前机器直接运行 Rust test harness 会出现 `STATUS_ENTRYPOINT_NOT_FOUND`。
- 但 `--no-run` 已证明测试二进制可编译产出。

### 2. formal insertion smoke

脚本：

- `openless-all/app/scripts/windows-real-asr-insertion-smoke.ps1`

用途：

- 证明正式 OpenLess 链路确实拿到 transcript、完成正式插入、写入 history，并能从目标控件读回。
- 当前机器未配置 ASR / LLM，因此通过 debug-only transcript bypass 验证正式链路：
  - `OPENLESS_DEBUG_TRANSCRIPT_FILE`
  - 仍然会经过正式 hotkey、session lifecycle、正式插入、正式 observer arm

Notepad 绿色基线：

```powershell
powershell -ExecutionPolicy Bypass -File .\openless-all\app\scripts\windows-real-asr-insertion-smoke.ps1 `
  -ExePath D:\cargo-targets\x86_64-pc-windows-gnu\debug\openless.exe `
  -Target notepad `
  -AsrProvider foundry-local-whisper `
  -InjectedTranscriptText "今天记录了几粒样本" `
  -AllowClipboardFallback
```

### 3. observer artifact verifier

脚本：

- `openless-all/app/scripts/windows-external-edit-observer-smoke.ps1`

用途：

- 绑定以下证据：
  - `correction-rules.json`
  - `openless.log`
  - summary JSON
- 当前版本已针对 Windows 做了两个验证层修正：
  - 不再高频轮询 `correction-rules.json`，避免干扰原子 rename 持久化
  - 支持 `openless.log` 被 smoke 删除后重新创建的场景

示例：

```powershell
powershell -ExecutionPolicy Bypass -File .\openless-all\app\scripts\windows-external-edit-observer-smoke.ps1 `
  -ExpectedPattern "粒" `
  -ExpectedReplacement "例" `
  -TimeoutSeconds 20 `
  -SummaryJsonPath .\.tmp\external-edit-evidence\notepad-summary-pass.json
```

### 4. real observer runbook

必须把“正式插入验证”和“observer 学习验证”分开理解：

- 插入 smoke 的最终断言要求目标文本仍等于本次 `finalText`。
- 如果在 observer 窗口内故意把目标文本从 `old` 改成 `new`，那么这个断言会故意失败。
- 所以 observer 学习验证时，`windows-real-asr-insertion-smoke.ps1` 的非零退出不能直接解读成产品失败。

正确做法：

1. 先单独跑 formal insertion smoke，证明正式链路能落字。
2. 再单独跑 observer verifier，同时在 observer 短窗口内修改目标术语。
3. 以 `correction-rules.json` 和 `[extedit] learned / persisted` 为 observer 成功证据。

## 当前已固定证据

### A. Notepad 正式插入成功

- 命令输出：
  - `.tmp/external-edit-evidence/notepad-rewrite-hit.log`
  - `.tmp/notepad-insertion-smoke-readback-pidfixed.log`
- 关键结果：
  - history 新增 session
  - `insertStatus=inserted`
  - Notepad UIA 读回成功

### B. Notepad observer 学习成功

- summary JSON：
  - `.tmp/external-edit-evidence/notepad-summary-pass.json`
- verifier stdout：
  - `.tmp/external-edit-evidence/notepad-observer-verifier-pass.log`
- 自动改词脚本日志：
  - `.tmp/external-edit-evidence/notepad-auto-correct-pass.log`
- 正式链路日志关键行：
  - `%LOCALAPPDATA%\OpenLess\Logs\openless.log`
  - `armed observer`
  - `learned rule 粒 -> 例`
  - `persisted rule id=...`
- 规则文件：
  - `%APPDATA%\OpenLess\correction-rules.json`
  - 当前已验证写入：`粒 -> 例`

### C. 后续正式输出命中成功

- 证据文件：
  - `.tmp/external-edit-evidence/notepad-rewrite-hit.log`
- 关键结果：
  - 新一轮正式 smoke 中 `rawTranscript` / `finalText` 已变成 `今天记录了几例样本`
  - `openless.log` 出现：
    - `[coord] correction rules adjusted raw transcript (9 → 9 chars)`

### D. Edge textarea 正式插入与 observer 学习成功

- formal insertion smoke：
  - `.tmp/edge-insertion-smoke-disabled-rule-instrumented.log`
  - `.tmp/external-edit-evidence/edge-formal-chain-pass.log`
- observer verifier：
  - `.tmp/external-edit-evidence/edge-observer-verifier-pass.log`
  - `.tmp/external-edit-evidence/edge-summary-pass.json`
- 自动改词日志：
  - `.tmp/external-edit-evidence/edge-auto-correct-pass.log`
- 正式链路日志关键行：
  - `%LOCALAPPDATA%\OpenLess\Logs\openless.log`
  - `armed observer`
  - `learned rule 粒 -> 例`
  - `persisted rule id=...`
- 关键结果：
  - Edge guest 窗口中的真实 textarea 读回成功
  - observer 在短窗口内识别人工改正并重新启用 `粒 -> 例`
  - `correction-rules.json` 已恢复为标准 JSON array 形态

### E. Edge 后续正式输出命中成功

- 证据文件：
  - `.tmp/external-edit-evidence/edge-rewrite-hit.log`
- 关键结果：
  - 新一轮正式 smoke 中 `rawTranscript` / `finalText` 已变成 `今天记录了几例样本`
  - Edge textarea readback 与 history 一致
  - `openless.log` 出现：
    - `[coord] correction rules adjusted raw transcript (9 → 9 chars)`

## 本次验证中的关键 expected vs actual

### 1. Notepad 读回失败

- expected：
  - smoke 脚本应从真实 Notepad 窗口进程读取 `RichEditD2DPT / Document` 文本。
- actual：
  - 旧脚本把 `Start-Process notepad.exe` 返回的 launcher pid 当成了窗口 pid，导致 UIA 读回进错进程，结果为空。
- 修复：
  - `windows-real-asr-insertion-smoke.ps1` 改为通过 `Wait-ProcessWindow "Notepad"` 锁定真实窗口进程。

### 2. Notepad 读回误触发热键

- expected：
  - smoke 读回不应再触发 OpenLess 自身热键。
- actual：
  - 旧 fallback 用 `Ctrl+A / Ctrl+C`，在当前热键配置为 `LeftControl` 时，会重新触发新 session。
- 修复：
  - Notepad 读回彻底移除 `Ctrl+A / Ctrl+C` fallback，改为 UIA `Document/ValuePattern` 轮询读取。

### 3. verifier 干扰规则持久化

- expected：
  - observer verifier 只观察证据，不干扰 `correction-rules.json` 持久化。
- actual：
  - 旧 verifier 高频轮询 `correction-rules.json`，会在 Windows 上干扰原子 rename，造成 `[extedit] persist learned rule failed: rename failed ...`
- 修复：
  - verifier 只记录 pre-state，轮询日志状态，结束后再读一次规则文件。

### 4. Edge 新 profile 抢焦点到 sync-confirmation 页面

- expected：
  - Edge smoke 应把正式插入与 observer 都落在 fixture textarea。
- actual：
  - 新 profile 首启时，Edge sync-confirmation 页面会抢走 focused element；observer baseline 读到的是 `edge://sync-confirmation-dialog/`，browser readback 也不再指向 textarea。
- 修复：
  - `windows-real-asr-insertion-smoke.ps1` 改为以 `--guest` 启动 Edge smoke 窗口，规避首启同步引导页。

### 5. Edge guest 窗口无法通过 `Get-Process.MainWindowHandle` 锁定

- expected：
  - browser smoke 应稳定拿到真实 Edge 顶层窗体并继续 UIA 聚焦 textarea。
- actual：
  - guest Edge 上 `Get-Process.MainWindowHandle` 不可靠，旧脚本会误判“Browser window process was not found”。
- 修复：
  - browser 窗口发现改为直接枚举 UIA 顶层窗体，并把真实 `pid/title/handle` 回填给后续 focus/readback 路径。

### 6. Windows 正式链路保存了不稳定 child HWND

- expected：
  - session 开始时捕获的 focus target 应该能在插入完成时仍可恢复。
- actual：
  - 旧逻辑直接保存 `GetForegroundWindow()` 返回的原始 HWND；在 Edge 上它可能是会销毁的 child 窗口，导致后续降级成 `CopiedFallback`。
- 修复：
  - `capture_focus_target()` 改为保存 `GA_ROOT` 根窗口 HWND，恢复前景时使用稳定顶层窗体。

### 7. `correction-rules.json` 被 PowerShell 包装对象污染后无法持久化

- expected：
  - 就算历史验证脚本把规则文件写成 `{ value: [...], Count: N }` 形态，正式 learning 也不应因为解码失败而丢掉本次学到的规则。
- actual：
  - observer 已经 `learned rule 粒 -> 例`，但 `CorrectionRuleStore::remember(...)` 在读取 wrapper 形态文件时解码失败，导致 persist 失败。
- 修复：
  - `CorrectionRuleStore` 新增 wrapped-array 兼容读取；下次正式写入时自动回正成标准 JSON array。

## 当前实现边界

- v1 terminology memory 物理文件就是 `correction-rules.json`
- v1 当前学到的是“最小 deterministic 替换 span”
  - 本次 Notepad 自动证据实际写入的是 `粒 -> 例`
  - 不是整词 `几粒 -> 几例`
- 不做云同步、导入导出、多设备合并
- 不扩 macOS observer
- 不把 Edge / WeChat / Zed / Windows Terminal 包装成“当前 worktree 已有同等级自动化证据”

## 变更边界

本次变更覆盖：

- Windows-only external-edit observer
- formal dictation pipeline arm / cancel / persist integration
- Settings 最小总开关
- 复用 Vocab correction rules 作为正式 terminology memory
- Windows 正式 smoke / observer verifier / runbook / 支持矩阵文档
