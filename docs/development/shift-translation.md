# 同段录音的 Shift 翻译开关

基线为 `504747c8`，复用[录音采集/后台任务](recording-overlap.md)与[加密 v1 次数设置](sync-v1-compatibility.md)。没有改变版本号、安装版本、真实用户数据或生产服务。

## 用户入口与兼容范围

桌面「通用 → 录音翻译」选择 Shift 连按次数，界面提供 1、2、3 次，默认一次；来自同步的其他合法次数也会保留并显示。开启和取消使用相同次数。每次新录音沿普通入口默认听写；同一段可以反复切换，不重启麦克风。

仅翻译快捷键仍为内置、不区分左右的 `Shift` 时启用新手势。已有自定义快捷键（包括自定义左/右 Shift）保留原来的“开启翻译”行为，不覆盖绑定，不额外执行 Shift 动作。Shift 本身作为录音键时优先录音，禁用内置翻译手势；Shift+其他键的录音组合依靠完整组合边沿排除，不禁止用户单独轻点 Shift。普通翻译不受 Ask/测试功能开关限制。

## 手势与采集身份

原生输入监听所有按下/松开边沿，先观察、再交给原快捷键分发，不吞掉 Shift 或鼠标事件。单次按住不超过 350ms，前一次松开到下一次按下不超过 400ms。只有完整、独立短按累计到设置次数才发送切换；左右交替可以累计，同时按两侧、repeat、长按、普通键/其他修饰键/可观察鼠标组合会清空本轮计数。macOS 观察原 CGEventTap 的鼠标按钮/滚轮和系统功能键；Windows 原键盘 hook 配套只观察的低层鼠标 hook，并在键盘边沿检查已按住的其他修饰键/鼠标按钮。

按下时的只读回调返回当前采集 `sessionId`、设置修订号和有效次数。上下文变化会丢弃半次手势和未完成序列；全局设置任意修订均重置计数，包括同步恢复、次数/快捷键修改。松键不能重新归属于新段，队列里的完成事件还会再检查设置修订号；Core 在同一把状态锁下核对 sessionId 与 Starting/Recording。停段、取消后或已进入后处理的请求均拒绝。没有延迟触发计时器，过期仅在后继输入时判断。

Starting 阶段尚未获得冻结上下文时，Core 保留原始翻译意图；上下文完成后重新计算 `translation_effective`。目标语言为空或与唯一工作语言相同时，胶囊继续显示普通模式；不存在仅修改前端选中状态的路径。原有语言选择菜单保持只允许当前有效翻译采集段修改，选择继续沿原规则记住到偏好；停止时封存本段最终目标语言。后处理只读 `submitted_dictation_context`，不重新读取当前全局模式或新段语言。

表达偏好指令在开始时冻结，随模式选择在当前上下文中加入/移除：最终普通听写采用冻结的指令，最终翻译不采用；关闭再打开可逆。未配置 LLM 时的原文回退仍优先。没有新增全局模式或第二套音频/目标/历史状态。

## 要求 → 入口 → 源码 → 验证

以下应用路径相对 `openless-all/app/`。

| 要求 | 实际入口 | 源码位置 | 验证 / 限制 |
| --- | --- | --- | --- |
| 完整短按，同次数双向切换 | macOS CGEventTap / Windows LL hooks | `src-tauri/src/hotkey.rs::observe_shift*`；`shift_translation.rs::ShiftTranslationGesture` | 独立 native harness 执行生产 macOS 字段分发与 1/2/3/255 次、组合/repeat/长按/双侧、设置变更、换段、失效窗口测试；Windows 条件测试留给 Windows runner。 |
| 自定义快捷键与录音键兼容 | 设置快捷键 → 原生分发 → 桥接 | `coordinator/hotkey_loops.rs::configure_shift_translation`、`TranslationShiftTapped` 分支，原 `arm_translation_if_effective` | 原自定义 modifier 回归保留；内置 Shift 动作仅通过 source 返回上下文启用；不依赖 Ask gate。macOS 宿主编译通过。没有声称物理键盘/Windows 全局注册实测。 |
| 当前段身份、Starting 和有效模式 | 原生完整手势 → Core 定向 toggle | Core `api.rs::toggle_dictation_translation_for_session` / `change_dictation_translation` | Starting 阻塞上下文夹具中开关三次后保留最终意图；旧 id、无有效目标、冗余语言回归；胶囊继续读取原后端 `translation_active`。 |
| 开后关音频完整，开后停产生翻译请求，旧段隔离 | 原生产 start/stop/submit → Pipeline/Router → OpenAI polisher | Core `recording_overlap_tests.rs::submitted_segments_release_capture_and_isolate_success_failure_and_cancel` | 六种成功/失败/取消与第二段已提交/仍采集交错；第一段同段开关后普通，第二段开启后翻译，核对两段所有首中尾 PCM 帧、真实序列化 system/user、历史模式、冻结目标和旧 id 拒绝。使用本机 HTTP/音频夹具，不是真实麦克风或云模型输出。 |
| 表达偏好对应最终模式 | capture → context toggle → 最终 prompt | Core `api.rs::capture_dictation_context*`、`dictation_context.rs::with_expression_instructions` / `restore_or_apply_unconfigured_llm_raw` | 真实请求断言普通段包含冻结表达指令、翻译段不包含；表达 consent/回退回归；翻译起录后取消也恢复开始时冻结的指令。 |
| 次数持久化/同步/显式默认 | 通用 → 录音翻译 → `update_setting_fields` | `src/features/settings/sections/ShiftTranslationSection.tsx`；`src/lib/types.ts`；已有 Core `UserPreferences` getter 和 Portable registry；`contract/backend-2.0.json::recordingTranslation` | React 控件 1→2→3→1 经原增量写入入口；既有可选字段/显式默认/零值拒绝同步回归通过；未重复扩展字段或加服务协议。旧版真客户端往返证据见同步依赖文档，本任务未重复运行该发布级矩阵。 |
| 多语言与页面可用 | 通用分类入口、选择控件与业务范围说明 | `navigation.ts` / `tabs.tsx`；`src/i18n/{zh-CN,zh-TW,en,ja,ko,es,fr,de}.ts` | TypeScript、八语言导航键与搜索回归、Chromium 真实 React 设置交互与截图；页面在隔离 fixture 使用生产组件，不能代替原生模式切换证据。 |

## 已执行的检查

- `cargo test --locked -p openless-core --lib translation_`：37 项通过。
- `cargo test --locked -p openless-core --lib expression_`：15 项通过。
- `cargo test --locked -p openless-core --lib submitted_segments_release_capture_and_isolate_success_failure_and_cancel`：六组交错通过，含表达偏好真实请求检查。
- `cargo test --locked -p openless-core --lib translation_requested_during_context_capture_is_session_scoped`：通过。
- `cargo test --locked -p openless-core --lib cloud_sync_e2ee_documents::tests::`：36 项通过，包含 `translationShiftTapCount` 的显式默认和无效值。
- `CARGO_TARGET_DIR=target cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml --test translation_hotkey`：macOS 原生分发、纯手势及既有快捷键回归共 47 项通过。
- `CARGO_TARGET_DIR=../target cargo check --locked --lib`（`src-tauri/`）：macOS 宿主通过，现有编译告警保留。
- `npx tsc --noEmit`、导航/八语言脚本、`npx vite build`：通过。
- `OPENLESS_E2E_PREVIEW_PORT=5551 OPENLESS_E2E_FIXTURE_PORT=5552 npx playwright test e2e/settings-shift-translation.spec.ts --project=chromium`：通过；截图在本工作区 `test-results/`，未提交生成物。

未执行安装包/真实硬件/真实模型、Windows 编译与设备验收，也未更改 Linux/iOS/Android/HarmonyOS 专用录音输入行为。后续需要平台验收时从正常原生入口复核左右 Shift、组合键和鼠标；无需用户变更已有快捷键。
