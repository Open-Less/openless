# Memory Graph — openless

- slug: openless
- path: `F:/编程/openless`
- updated: 2026-09-18

## Summary

OpenLess：Tauri 2 + Rust + WebView 听写/选区助手。上游 Open-Less/openless，维护者克隆 HKLHaoBin/openless。

## Entities

- SelectionVoice (Feature): 选区语音问答/编辑，EditPlan 结构化改写
- EditPlan (Module): XML/JSON 操作计划，本地确定性 apply
- StylePack (Entity): 含 prompt / selectionPrompt / voiceEditPrompt
- Issue1076 (Issue): EditPlan 解析失败（模型输出正文）
- Issue1046 (Issue): 历史页对已完成转录的录音提供重新转录与试听
- Issue1081 (Issue): 激活后立即打开麦克风、避免首字丢失，并评估文件式 ASR
- QuickNote (Feature): 速记，统一历史中的独立记录类型；永久保留录音，支持播放、导出、重转写与重润色
- QuickNoteCapture (Workflow): Android 先开始未定类录音，结束时由普通点击或方向手势决定普通听写/速记

## Relations

- SelectionVoice --uses--> EditPlan: 编辑意图生成方案后替换选区
- SelectionVoice --reads--> StylePack.voiceEditPrompt: 空则 prefs 自定义，再则默认 XML/JSON prompt
- EditPlan --parses-with-priority--> Xml|Json: 用户选择优先格式，另一种兜底
- QA Panel --shows--> model_output: 解析失败时展示原始模型输出
- History --retranscribes--> archived_recording: 有归档 WAV 的传统 ASR 条目可用当前 provider 重转
- OpenLess --tracks--> Issue1081: 上游 Issue，关联 Core 2.0 的录音启动等待与非流式 ASR 方案
- QuickNote --belongs-to--> History: 速记必须在统一历史记录中留痕，但拥有独立的音频生命周期
- QuickNoteCapture --classifies-at-stop--> QuickNote: Android 录音开始时无法预判意图，速记手势在结束时立即生效
- QuickNote --uses--> AudioArchive: 从录音第一帧开始持久化，转写失败时仍可播放、导出和重转写

## Facts

- 2026-09-15：扫描确认该项目包含 `.cursor`，已纳入本次全局图谱一致性更新。
- 2026-09-14 开分支 fix/selection-voice-editplan-prompt-format（基于 upstream/beta）
- Issue: https://github.com/Open-Less/openless/issues/1076（完全解决前不提 PR）
- PR: https://github.com/Open-Less/openless/pull/1077（目标 beta，关联并关闭 #1076）
- 根因：听写润色 user framing「只输出正文」与 EditPlan system prompt 冲突
- folia-major 参考：OUTPUT CONTRACT + 剥围栏/平衡括号候选解析
- 2026-09-15 基于 upstream/beta 创建 fix/1046-history-retranscription；历史页将重新转录入口从失败状态扩展到所有有归档录音的传统 ASR 条目，继续保留多模态能力边界
- 2026-09-15 提交 c038676e 并推送至 origin/fix/1046-history-retranscription；review-bugbot 复审结论为无 bug，CI、Android APK 与跨平台桌面发布构建均成功
- 2026-09-15 已下载 Android 四架构 APK 与 Windows/macOS 桌面产物；APK ZIP、Updater JSON、macOS updater tar.gz 及文件完整性静态校验全部通过，因无连接 ADB 设备未执行真机安装
- 2026-09-15 全局扫描确认项目根目录已有 `memory-graph.md`，纳入按项目名路由表；当前工作区仍有未提交的 vendor 修改
- 2026-09-15 向上游 `Open-Less/openless` 的 `beta` 提交 PR #1079；克隆仓库误建的 PR #6 已关闭，源分支仍为 `HKLHaoBin:fix/1046-history-retranscription`
- 2026-09-16 Android 日志诊断：历史录音 WAV 可正常读取（3,519,088 bytes），但同一 Bailian ASR 收尾路径在录音停止后约 12 秒报 `final result timed out`；移动端 History 在详情页打开时只把 `actionError` 渲染在隐藏的列表面板，因而重转失败可能表现为“无报错、无成功”。
- 2026-09-16 Android 悬浮窗诊断：多次 `cpal` 回调持续收到非零音频并正常释放麦克风，录音链路本身可用；`08:32:30` 的 `start_dictation failed` 直接原因是 Bailian WebSocket 在 5 秒上限内连接超时。`cpal Stream pause before drop failed` 只是停止时的清理警告。悬浮窗红色描边表示录音中，红色底色表示错误态；原生启动调用是 fire-and-forget，启动失败后的错误视觉可能保留。
- 2026-09-16 克隆仓库 `beta` 快进同步上游 `Open-Less/openless/beta` 至 `9b55ae1a`，其中包含百炼 WebSocket 优先 IPv4 修复 `af9eaa3`；克隆发布准备提交为 `07ce1e9a`。
- 2026-09-16 克隆仓库发布 `v2.0.0-Beta.2-tauri`（版本 `2.0.0-Beta.2`，GitHub prerelease）；Android 与桌面发布工作流均成功，Android 四架构 APK、签名和 Beta 更新清单已上传。发布工作流改用 `${{ github.repository }}` 生成克隆仓库清单 URL；旧版手机首次切换仍需手动覆盖安装 APK。
- 2026-09-17 启动延迟诊断：Android 与 Windows 共用的 Core 2.0 录音管线在 `fc38edd1` 重构后变为“先等待 `TranscriptionEngine::start`（云 ASR 建立新 WebSocket），再调用 `AudioRecorder::start`”；因此每次点击都会把 DNS/TCP/TLS/WS 建连放在麦克风启动前。旧协调器则先开 Recorder，再用 `DeferredAsrBridge` 缓冲等待 ASR 的音频。当前日志中 Windows `ToggleDictation` 到 `inputDevice` 约 0.3–0.8 秒，`inputDevice` 到首个 cpal 回调约 0.6 秒；Android 首回调约 0.3–0.5 秒。非零 PCM 持续到达，说明麦克风/权限不是主因；`cpal Stream pause before drop failed` 是停止时清理警告。
- 2026-09-17 已向上游 Open-Less/openless 提交 Issue #1081（https://github.com/Open-Less/openless/issues/1081），描述先开麦、首字保留、当前 partial 不可见及文件式 ASR 备选方案；标签补加因账号缺少上游 `AddLabelsToLabelable` 权限失败。
- 2026-09-18 讨论 Issue #1029（https://github.com/Open-Less/openless/issues/1029）形成速记方案边界：所有听写仍进入统一历史；速记是独立记录类型而非独立用户可见历史；暂不自动追加 Markdown；用户可选择当前或其他风格包；重润色先预览、应用后更新正文；速记录音永久保留至用户手动删除。
- 2026-09-18 Android 速记流程确定为“先普通点击开始、结束时普通点击=普通听写，配置方向手势=速记”，因此录音意图在开始时未定，必须从第一帧开始落盘；桌面端使用独立快捷键并复用现有快捷键组件；桌面可指定音频路径，Android 通过分享/文件导出。

## Decisions

- 用户可选 EditPlan 输出优先 XML 或 JSON；解析双向兜底
- 提示词：设置自定义 > 风格包 voiceEditPrompt > 内置默认
- 失败错误保留 ---model_output--- 供 QA 面板展示
- 重新转录按钮不改变录音归档隐私策略：只有 `hasAudioRecording` 为 true 且非多模态条目展示，成功/润色失败/转录失败均可使用
- 速记音频不受 `recordAudioForDebug` 开关影响；普通听写仍可按原录音保留策略处理，速记的音频清理仅由用户手动删除触发
- Android 四个方向在设置中作为可配置动作槽；默认行为需兼容现有翻译、QA 收尾和取消录音语义，速记手势在录音结束时分类当前会话
- 速记不自动追加 Markdown；正文可按用户选择的风格包生成，重润色结果应用后写回速记记录
