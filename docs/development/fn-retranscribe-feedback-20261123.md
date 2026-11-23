# 653816 DEV 的 Fn 接续与历史重新转录反馈

基线为 653816ac72d6693e71238989bfe41c6f1ede8fa3。本修复不改版本、安装应用或 PR1487；不调用真实麦克风或外部识别/润色服务。普通输入规则以用户本轮明确纠正为准：普通听写向最终交付时的当前焦点新增文字，只有改写既有选区/正文才校验原控件、范围和原文。

## 已读取的真实证据

实际查看了 12.27.42 录屏全部六张逐秒抽帧、12.28.47 历史菜单画面，以及 12.33.36 的翻译条/原文轨道抽帧（全部五页）。第一条约 32 秒显示原文、33 秒显示错误、35 秒显示“输入或设置无效”；第二条明确是历史菜单重新转录失败。抽帧位于 next-dev-macos 工作区 target/feedback-media-20261123，未复制原始媒体或用户历史进仓库。

仅沿源码确定的 ~/Library/Logs/OpenLess/openless.log 和 ~/Library/Application Support/OpenLess/preferences.json、history.json、对应 WAV 只读取证。只读取相关记录与录音元数据，不读取凭据库。日志为 UTC，以下时刻转换为本地 UTC+8。

| 本地时间 / 日志行 | 事实 | 能证明什么 |
| --- | --- | --- |
| 12:27:14.488–14.519，4268–4273 | dd344d23 发送 46422 PCM 字节，ASR 返回 0 字，随后报 polish input must not be empty / InvalidArgument | 空 ASR 被错误送进润色；并非服务 HTTP 401 |
| 12:28:16.624–16.803，4392–4397 | 7cf737ee 发送 30038 PCM 字节，ASR 142ms 返回 0 字；历史为 polishFailed、润色 0ms | 对应用户所选 0.7 秒失败记录；Ark 标签来自冻结上下文，不能证明调用过 Ark |
| 12:28:34–35 / 52–53，4398–4405 | 两次历史重转都连接 Volcengine 并发送同样 30038 字节；无连接失败或服务错误记录 | 历史按钮已到达真实 ASR；原代码在空结果后返回固定中文，前端却丢弃原因只显示“操作失败” |
| 12:28:14–15，4364–4376 | 长段发送 1463638 字节，ASR 158 字，所选导入风格“11111”润色输出 3 字，随后旧目标拒写 | 原文不是全部丢失，模型请求成功且风格不同；不能把 3 字输出或目标拒写混称转录掉字 |
| 12:33:26–28，5011–5025 | 59178ms PCM，ASR 243 字、润色 178 字成功，最终 native keyboard delivery = NoProgress，Core = PasteSent | 粘贴发出与实际输入成功不是同一个回执；原来确认前已恢复剪贴板 |
| 12:34:40–42，5146–5159 | 69258ms PCM，ASR 238 字、润色 156 字成功，之后 original input target or position changed 拒写 | 用户新规则要求取消普通输入的这条旧目标限制，保留选区改写校验 |

对应短段 WAV 均为合法 16kHz/单声道/16-bit PCM：dd344d23 为 23211 帧、1.4506875 秒，7cf737ee 为 15019 帧、0.9386875 秒，两者 peak 均为 8/32768；发送字节数与文件 PCM 完全一致。它们不是损坏/零字节文件，但保存的声音极弱。不能据此认定用户没说话，也不能承诺从文件恢复没有保存下来的语音；本轮未把这些录音送到真实服务重试。

当前只读偏好：Fn / Auto、traditional、Volcengine、streamingInsert=true、recordAudioForDebug=true、lowLatencyDictationEnabled=false、audioEnhancementEnabled=false、MacBook Air 麦克风。没有修改用户偏好。

## 修改与因果边界

- api.rs 在设备确认停止并提交后台任务时改用 hotkey.submitted()：清除旧段物理代次和已消费的防抖，不再执行 450ms 终态冷却。原来这段冷却确实会拒绝立即补充的下一次 Fn；真实日志没有物理 pressId 记录，因此不声称它解释所有用户按键。取消/真正未提交终态继续保留原规则，迟到 key-up 无权停止新段。
- Pipeline 在空/全空白 ASR 时跳过润色，让正常 Core 空稿出口生成 emptyTranscript 历史、零模型调用和 empty_transcript 状态。不是把失败改成成功。
- Tauri 普通听写捕获应用只用于提示和风格路由，延迟至最终处理锁内确定输入目标。不再用普通听写的旧控件/范围拒写或复原旧 App。Core 对延迟准备的插入资源关闭组合输入和目标分块写入，Mac/Windows 的最终输入继续由同一个处理锁串行；胶囊实时 ASR 不变。选区服务生产代码未改。
- macOS 普通粘贴的 AX 交付确认提前到原有 clipboard delivery gate 内，确认之后才按偏好恢复旧剪贴板。未确认时保持 PasteSent，不假称已输入，也保留完整结果供手动粘贴；新用户复制仍优先。明确复制回退须在同一锁内实际写入并读回成功，才返回复制成功。
- 历史命令复用 decode_dictation_wav，拒绝无效/截断 WAV，正确跳过合法元数据块。正常命令调用的实现单独放在 history_retranscription.rs，使回归沿同一读取、当前 ASR、持久化路径执行。旧版空原文且空终稿的 polishFailed 可在真实重转有文字后原位修复；真正润色失败而原文存在的记录仍只展示临时结果。
- 宿主只返回有界错误原因，不发送服务正文/密钥。onRetranscribe 单独映射原因，未改复制操作或 History/List/Detail。新增八语言错误文案。
- Fn 接受的 pressId/短按时长与独立 Shift 对应的 session/result 增加安全诊断。现有 Shift 完整按下松开、次数、组合/长按排除逻辑保留；旧日志不能定位每一次未响应的物理 Shift，不能宣称偶发硬件手势已真机修复。

## 验证

两条确定性失败都做了反例：临时恢复旧 submitted→terminal 后，正常 Fn press/release 的下一段在 100ms 衔接时被拒绝，测试明确报 rapid successor was lost；临时恢复旧空稿润色条件后，正常 start/stop + Pipeline 返回 InvalidArgument，而不是预期 Provider/emptyTranscript。已恢复修复并运行对应成功检查，没有修改用户数据或旧 Beta。

- Core dictation_target_capture：6 项通过，包含完整 Fn Auto press/release、100ms 快速接续、生产 Pipeline/Router、两段不同首中尾 PCM、翻译冻结、前段结束时后段仍录音、移动到新 App/光标后的当前焦点交付，以及真实本机 HTTP 请求保持开始时风格。测试端口为受控音频/编辑器，非物理麦克风或 AX/UIA 真机。
- rapid_three_submissions_keep_pcm_and_join_final_input_before_the_next：首段延迟成功/失败/取消三场景通过，三段首中尾 PCM 独立，实际插入调用严格 start1→end1→start2→end2→start3→end3（失败/取消首段零写入），第二、三段历史独立。首段插入阻塞时后段不得开始写。
- empty_transcript_persists_a_failed_history_entry_without_activity：正常 Pipeline 空白 ASR + 已配置结构化润色正反例通过，润色调用数零、emptyTranscript 历史、无虚构 LLM 用量。
- history_command_decodes_audio_repairs_legacy_empty_failure_and_preserves_errors：生产宿主实现 + 真实 Core AuxiliaryService 通过有效音频/元数据块、旧空稿修复、空 ASR、服务错误、截断和缺失文件、已完成记录不覆盖；失败历史与原 WAV 不变。
- clipboard_delivery：10 项受控生产事务回归通过，覆盖未知粘贴保留、下一段借用/恢复、新用户复制优先、取消/目标变化和假写入回执拒绝；独立 Shift 生产手势模块 4 项通过。
- 显式运行 native_clipboard_fallback_is_readable_and_not_restored_after_an_unconfirmed_paste：macOS 实际 arboard 写入/读回、800ms 后完整文本、原生 changeCount、迟到恢复不覆盖新复制通过；不发送粘贴按键，测试后明确恢复原文本剪贴板。它证明真实系统剪贴板，不证明真实目标 App 的键盘交付。
- WebKit 实际历史菜单点击“重新转录”后显示明确空 ASR 原因，记录保留；TypeScript 与错误映射检查通过。
- macOS 宿主 cargo check 通过，保留既有原生编译警告；共享 Core cargo clippy --all-targets -- -D warnings 通过。

未操作/启动/退出安装应用，未真实录麦克风、未发收费或外部模型请求，未做 Windows 真机或发布打包。新的 DEV 还需协调者整合后由用户复测物理 Fn/Shift、真实输入框及真实识别质量；不以受控两段或 CI 代替这部分验收。

## UI 交接边界

用户随后把 UI 交给 Gemini。此前完成的 useHistoryEntryActions::onRetranscribe、安全原因映射、八语言文案及 WebKit 菜单回归保留为独立附属提交，之后没有再改前端、CSS、renderer 或 tauri_events。Core/宿主提交可独立整合，但最终用户反馈须由 Gemini 沿 historyRetranscribeNoSpeech/Missing/InvalidAudio/Permission/Active/Busy/Unsupported/Configuration/Storage/Provider/InvalidId 契约接通；empty_transcript 胶囊映射由原胶囊 owner 交接。
