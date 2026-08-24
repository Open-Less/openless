// TypeScript mirror of src-tauri/src/types.rs.
// All keys are camelCase (Rust serializes with #[serde(rename_all = "camelCase")]).
// PolishMode is an exception — Rust uses lowercase serialization.

import type {
  AndroidAccessibilityStatus,
  AndroidInsertStrategy,
  AndroidOverlayActivationMode,
  AndroidOverlayCancelSwipeDirection,
  AndroidOverlayLeftSwipeAction,
  AndroidOverlayStatus,
  AndroidOverlayTrigger,
} from '../../android/frontend/lib/androidTypes';

export type {
  AndroidAccessibilityStatus,
  AndroidInsertStrategy,
  AndroidOverlayActivationMode,
  AndroidOverlayCancelSwipeDirection,
  AndroidOverlayLeftSwipeAction,
  AndroidOverlayStatus,
  AndroidOverlayTrigger,
};

export type PolishMode = 'raw' | 'light' | 'structured' | 'formal';

/** 识别管线模式（issue #902）：traditional = ASR + LLM 两段式；
 *  multimodal = 单个多模态模型一步完成「音频 + 提示词 → 最终文本」。
 *  两套配置在凭据库中完全隔离，运行时只读当前模式。 */
export type PipelineMode = 'traditional' | 'multimodal';

export type InsertStatus = 'inserted' | 'pasteSent' | 'copiedFallback' | 'failed';

export type DictionaryDeliveryMode = 'dedicated' | 'prompt' | 'unsupported' | 'multimodalPrompt';

export interface DictionaryDeliveryReport {
  provider: string;
  mode: DictionaryDeliveryMode;
  sentCount: number;
  droppedCount: number;
  reason: string | null;
}

/** 概览页年度活动热力图的单日计数（date = 本地日期 YYYY-MM-DD）。 */
export interface ActivityDay {
  date: string;
  count: number;
  /** 当日最终插入文本的总字符数。升级前写入的日期没有这个字段（读作 0）。 */
  chars?: number;
  /** 当日录音总时长（毫秒）。升级前写入的日期没有这个字段（读作 0）。 */
  durationMs?: number;
}

export interface DictationSession {
  id: string;
  createdAt: string; // ISO-8601
  rawTranscript: string;
  /** 纠正规则**之前**的 ASR 原文。`rawTranscript` 存的是规则跑完之后的版本，
   *  两者相同时后端不写这个字段（null）。用于归因：一次误识别到底是 ASR 听错还是
   *  LLM 改坏。旧历史没有此字段。 */
  asrTranscript: string | null;
  finalText: string;
  mode: PolishMode;
  stylePackId: string | null;
  translationActive: boolean;
  polishSource: string | null;
  appBundleId: string | null;
  appName: string | null;
  insertStatus: InsertStatus;
  errorCode: string | null;
  durationMs: number | null;
  dictionaryEntryCount: number | null;
  /** 该会话是否在录音时归档了原始 wav（取决于当时 prefs.recordAudioForDebug）。
   *  true 时前端在 History 渲染播放按钮，凭 id 通过 read_audio_recording IPC 拿字节流。 */
  hasAudioRecording: boolean | null;
  /** 本次转写用的 ASR provider id（如 "volcengine" / "local-qwen3"）。旧历史为 null。 */
  asrProvider: string | null;
  /** 本次转写用的 ASR 模型 id。provider 无模型概念时为 null。 */
  asrModel: string | null;
  /** 本次润色用的 LLM provider id。Raw 直通（未调用 LLM）时为 null。 */
  llmProvider: string | null;
  /** 本次润色用的 LLM 模型 id。Raw 直通时为 null。 */
  llmModel: string | null;
  /** 本次会话走的识别管线模式（"multimodal" / 缺失 = 传统两段式）。 */
  pipelineMode?: string | null;
  /** 松键后等待转写结果的实测耗时（毫秒）。流式 ASR 是收尾延迟，批式是完整转写耗时。 */
  asrMs: number | null;
  /** LLM 润色/翻译调用的实测耗时（毫秒）。未调用 LLM 时为 null。 */
  polishMs: number | null;
  /** 本次 ASR 请求实际如何投递本地词典；旧历史没有该字段。 */
  asrDictionaryDelivery?: DictionaryDeliveryReport | null;
  /** 本次 LLM prompt 实际携带的词条数量。 */
  llmDictionarySentCount?: number | null;
  llmDictionaryDelivery?: DictionaryDeliveryReport | null;
}

export type DictionarySource = 'manual' | 'learned' | 'imported';
export type DictionaryScope = 'persistent' | 'temporary';

export interface DictionaryEntry {
  id: string;
  phrase: string;
  note: string | null;
  enabled: boolean;
  hits: number;
  createdAt: string;
  source: DictionarySource;
  scope: DictionaryScope;
  pinned: boolean;
  lastHitAt: string | null;
  expiresAt: string | null;
  projectKey: string | null;
}

/** 一条纠正规则是怎么来的。老的 correction-rules.json 没有这个字段，后端反序列化时
 *  落到 'manual'——那些确实都是手动加的。 */
export type RuleSource = 'manual' | 'learned';

export interface CorrectionRule {
  id: string;
  pattern: string;
  replacement: string;
  enabled: boolean;
  createdAt: string;
  source: RuleSource;
}

/** `debug_read_cursor_context` 的返回：一次光标上下文探测的完整结果。
 *  status 之外的每一种都要能说清「为什么没读到」——装机验证时全靠它判断某个 app
 *  是被安全闸门拦住了，还是 AX 根本不支持。 */
export interface HostDocumentReadResult {
  status: 'ok' | 'blocked' | 'unsupported' | 'unavailable' | 'timeout';
  reason: string | null;
  window: { text: string; cursor: number } | null;
  appName: string | null;
  bundleId: string | null;
  elapsedMs: number;
}

/** 一条等待用户确认的纠正建议；收件箱开启时可跨重启保留到 expiresAt。 */
export interface PendingCorrection {
  id: string;
  pattern: string;
  replacement: string;
  confidence: 'high' | 'low';
  attribution: 'asr' | 'llm' | 'unknown';
  createdAt: string;
  expiresAt: string | null;
  sourceApp: string | null;
}

/** 为什么这段话没落进目标 app。只用于后端日志，卡片本身不渲染它。 */
export type InsertFallbackReason = 'partialStream' | 'insertFailed';

/** 落字失败兜底卡片的内容。`text` 始终是完整的那段话，即便屏幕上只落了半截。 */
export interface InsertFallbackCardPayload {
  text: string;
  reason: InsertFallbackReason;
  /** 本次展示代次；尺寸回报必须原样携带，后端据此忽略旧卡片的迟到 IPC。 */
  presentationId: number;
}

export interface VocabPreset {
  id: string;
  name: string;
  phrases: string[];
}

export interface VocabPresetStore {
  custom: VocabPreset[];
  overrides: VocabPreset[];
  disabledBuiltinPresetIds: string[];
}

export type HotkeyTrigger =
  | 'rightOption'
  | 'leftOption'
  | 'rightControl'
  | 'leftControl'
  | 'rightCommand'
  | 'leftCommand'
  | 'leftShift'
  | 'rightShift'
  | 'fn'
  | 'rightAlt'
  | 'mediaPlayPause'
  | 'custom';

export type HotkeyMode = 'toggle' | 'hold' | 'doubleClick' | 'auto';

export interface HotkeyKey {
  code: string;
}

export interface HotkeyBinding {
  trigger: HotkeyTrigger;
  mode: HotkeyMode;
  keys?: HotkeyKey[] | null;
}

export type HotkeyAdapterKind = 'macEventTap' | 'windowsLowLevel' | 'fcitx5' | 'unavailable';

export interface HotkeyCapability {
  adapter: HotkeyAdapterKind;
  availableTriggers: HotkeyTrigger[];
  requiresAccessibilityPermission: boolean;
  supportsModifierOnlyTrigger: boolean;
  supportsSideSpecificModifiers: boolean;
  explicitFallbackAvailable: boolean;
  statusHint: string | null;
}

export interface HotkeyInstallError {
  code: string;
  message: string;
}

export type HotkeyStatusState = 'starting' | 'installed' | 'failed';

export interface HotkeyStatus {
  adapter: HotkeyAdapterKind;
  state: HotkeyStatusState;
  message: string | null;
  lastError: HotkeyInstallError | null;
}

export interface ShortcutBinding {
  /** 主键，例如 "D" / "Space" / "F1" / "RightOption" / "LeftShift" */
  primary: string;
  /** 修饰符：泛化 tag（cmd/ctrl/…）或侧别 tag（cmd-left/ctrl-right/…）。 */
  modifiers: string[];
}

/** 风格包直达快捷键：binding 按下即激活 packId 对应的风格包（issue #759）。 */
export interface StylePackHotkey {
  packId: string;
  binding: ShortcutBinding;
}

/** 划词语音问答快捷键绑定。null 表示未启用。详见 issue #118。 */
export type QaHotkeyBinding = ShortcutBinding;

/** 自定义录音组合键绑定。当 hotkey.trigger == 'custom' 时使用。 */
export type ComboBinding = ShortcutBinding;

export type CodingAgentProviderId =
  | "claude-code-cli"
  | "opencode-cli"
  | "codex-cli"
  | "dsh-cli";
export type CodingAgentPermissionMode =
  | "plan"
  | "default"
  | "acceptEdits"
  | "bypassPermissions";

/** 模拟粘贴时按下的快捷键。仅 Windows/Linux 生效；macOS 走 AX 直写。
 *  - ctrlV       : 标准粘贴（默认；大多数编辑器、浏览器、IDE）
 *  - ctrlShiftV  : kitty / alacritty / wezterm / gnome-terminal / foot 等终端
 *  - shiftInsert : xterm / urxvt 等老派 X11 终端
 *  详见 issue #360。 */
export type PasteShortcut = 'ctrlV' | 'ctrlShiftV' | 'shiftInsert';

/** Windows 听写文本插入策略。 */
export type WindowsInsertionMode = 'tsf' | 'sendInput' | 'paste';

/** Windows SendInput 路径的换行模拟方式。 */
export type WindowsSendInputNewlineMode = 'enter' | 'shiftEnter' | 'crlf';

/** macOS 逐字上屏时换行符怎么发。`auto` 按前台应用选择；`lineFeed` 供终端使用；
 *  `return` 在聊天框里等于发送 —— 靠换行拆多条消息的风格包要的就是这个。 */
export type MacosNewlineMode = 'auto' | 'shiftReturn' | 'lineFeed' | 'return';

export type WindowsImeInstallState =
  | 'installed'
  | 'notInstalled'
  | 'registrationBroken'
  | 'notWindows';

export interface WindowsImeStatus {
  state: WindowsImeInstallState;
  usingTsfBackend: boolean;
  message: string;
  dllPath: string | null;
}

/** 后台自动更新渠道。stable = 查正式版 manifest（默认）；beta = 查
 *  latest-android-{arch}-beta.json。手动「检查正式版/Beta 更新」按钮不受此字段影响。 */
export type UpdateChannel = 'stable' | 'beta';

export type ThemeMode = 'system' | 'light' | 'dark';

/** 选区润色结果直接替换，或先在可编辑预览中确认。 */
export type SelectionPolishOutputMode = 'directReplace' | 'previewConfirm';

export interface CustomStylePrompts {
  raw: string;
  light: string;
  structured: string;
  formal: string;
}

export interface StyleSystemPrompts {
  raw: string;
  light: string;
  structured: string;
  formal: string;
}

export type StylePackKind = 'builtin' | 'imported';

export interface StylePackExample {
  title?: string | null;
  input: string;
  output: string;
}

export interface StylePack {
  id: string;
  name: string;
  description: string;
  author?: string | null;
  version: string;
  kind: StylePackKind;
  baseMode: PolishMode;
  /** For selected written text. Empty values in legacy packs use a safe backend default. */
  selectionPrompt: string;
  prompt: string;
  examples: StylePackExample[];
  tags: string[];
  iconPath?: string | null;
  createdAt?: string | null;
  updatedAt?: string | null;
  enabled: boolean;
  active: boolean;
  recommendedModel?: string | null;
  compatibleAppVersion?: string | null;
  /** 衍生关系：null = 本地原创（或还没首发到云端）；非空 = 这份 pack 安装自云端 originPackId。 */
  originPackId?: string | null;
  originAuthorLogin?: string | null;
}

export interface StylePackRuntimeDiagnostics {
  packId: string;
  packName: string;
  packPrompt: string;
  packPromptChars: number;
  contextPremise: string;
  contextPremiseChars: number;
  hotwordBlock: string;
  hotwordBlockChars: number;
  historyInstruction: string;
  historyInstructionChars: number;
  singleTurnPrompt: string;
  singleTurnPromptChars: number;
  multiTurnPrompt: string;
  multiTurnPromptChars: number;
  workingLanguages: string[];
  hotwords: string[];
  contextWindowMinutes: number;
  includesContextPremise: boolean;
  includesHotwordBlock: boolean;
  includesHistoryInstruction: boolean;
  previewOmitsFrontApp: boolean;
}

export interface UserPreferences {
  hotkey: HotkeyBinding;
  dictationHotkey: ShortcutBinding;
  defaultMode: PolishMode;
  enabledModes: PolishMode[];
  activeStylePackId: string;
  styleSystemPrompts: StyleSystemPrompts;
  customStylePrompts: CustomStylePrompts;
  launchAtLogin: boolean;
  showCapsule: boolean;
  /** 录音胶囊样式（'siri' | 'classic'）。见 CapsulePayload.capsuleStyle 的运行时下发。 */
  capsuleStyle: CapsuleStyle;
  /** 录音期间临时静音系统输出，停止/取消/出错后恢复原静音状态。 */
  muteDuringRecording: boolean;
  /** 按下录音热键进入 recording 状态时，播放一段合成提示音提醒「已开始录音」。
   *  默认开启；在 capsule 窗口用 Web Audio API 合成，不依赖 showCapsule。 */
  audioCueOnRecord: boolean;
  /** Toggle 模式「说完自动停止」（issue #860）。默认关闭；开启后检测到语音、
   *  连续静音达到 silenceAutoStopSeconds 时自动停止并提交，一直没说话则 10 秒后取消。 */
  silenceAutoStopEnabled: boolean;
  /** 语音后的连续静音阈值（秒）。可选 1 / 1.5 / 2 / 3 / 4 / 5，默认 3。 */
  silenceAutoStopSeconds: number;
  /** 录音输入设备名称。空字符串 = 使用系统默认麦克风。 */
  microphoneDeviceName: string;
  activeAsrProvider: string;
  activeLlmProvider: string;
  /** 识别管线模式（实验性，issue #902）。multimodal 时各语音管线改用 omni 配置。 */
  pipelineMode: PipelineMode;
  /** 「多模态识别管线」实验性功能总开关（高级设置）。默认 false。 */
  multimodalPipelineEnabled: boolean;
  /** 多模态（Omni）模型当前激活的 provider id，镜像凭据库 omni.active。 */
  activeOmniProvider: string;
  /** LLM 思考模式开关。默认关闭；OpenAI 普通 chat 模型会跳过不支持的字段。详见 issue #402。 */
  llmThinkingEnabled: boolean;
  /** 是否使用系统代理（issue #869）。默认开启；关闭后所有请求直连，境外服务（GitHub 登录/更新等）可能连不上。 */
  useSystemProxy: boolean;
  /** 仅 Windows/Linux：粘贴成功后是否恢复用户原剪贴板。默认 true。详见 issue #111。 */
  restoreClipboardAfterPaste: boolean;
  /** 仅 Windows/Linux：模拟粘贴时按下的快捷键。详见 issue #360：kitty/alacritty
   *  等终端只接受 Ctrl+Shift+V，硬编码 Ctrl+V 会被吞掉，听写文本只剩在剪贴板里。
   *  macOS 走 AX 直写不受影响。默认 'ctrlV' 与历史行为一致。 */
  pasteShortcut: PasteShortcut;
  /** Windows：TSF 失败后是否允许快捷键粘贴 / 剪贴板兜底。仅在剪贴板写失败时才再试 SendInput。关闭后可验证是否真实 TSF 上屏。 */
  allowNonTsfInsertionFallback: boolean;
  /** Windows：听写插入策略（TSF / SendInput / 剪贴板粘贴）。 */
  windowsInsertionMode: WindowsInsertionMode;
  /** Windows SendInput 路径的换行模拟方式。 */
  windowsSendInputNewlineMode: WindowsSendInputNewlineMode;
  macosNewlineMode: MacosNewlineMode;
  /** 旧版兼容：`true` 等价于 `windowsInsertionMode === 'sendInput'`。 */
  windowsSendInputInsertionOnly: boolean;
  /** Windows：SendInput 模式下是否在系统键盘列表（Win+Space）中显示 OpenLess。 */
  windowsShowOpenlessInKeyboardList: boolean;
  /** 用户的工作语言（多选，原生名）；作为前提注入 LLM polish/translate prompt 头部。 */
  workingLanguages: string[];
  /** 翻译模式目标语言（单选，原生名）；空串 = 不启用 Shift 翻译。详见 issue #4。 */
  translationTargetLanguage: string;
  /** 中文输出字形偏好：由界面语言（简/繁）自动同步，不单独暴露设置项。 */
  chineseScriptPreference: 'auto' | 'simplified' | 'traditional';
  /** 最终输出语言偏好：由界面语言自动同步，不单独暴露设置项。 */
  outputLanguagePreference: 'auto' | 'zhCn' | 'zhTw' | 'en' | 'ja' | 'ko';
  /** 划词语音问答快捷键。null = 未启用。详见 issue #118。 */
  qaHotkey: QaHotkeyBinding | null;
  /** 选区润色快捷键。null = 已停用。 */
  selectionPolishHotkey: ShortcutBinding | null;
  /** The style pack used only by selected written-text polishing. */
  selectionPolishStylePackId: string;
  /** 选区润色结果的交付方式。 */
  selectionPolishOutputMode: SelectionPolishOutputMode;
  /** 是否把 Q&A 历史写到本地存档。详见 issue #118。 */
  qaSaveHistory: boolean;
  /** 自定义录音组合键。当 hotkey.trigger == 'custom' 时使用。null = 未设置。 */
  customComboHotkey: ComboBinding | null;
  /** 录音中触发翻译的全局快捷键。默认 Shift。 */
  translationHotkey: ShortcutBinding;
  /** 切换到上一个润色风格的全局快捷键。null = 用户已停用（issue #576）。 */
  switchStyleHotkey: ShortcutBinding | null;
  /** 打开 OpenLess 主窗口的全局快捷键。null = 用户已停用（issue #576）。 */
  openAppHotkey: ShortcutBinding | null;
  /** 风格包直达快捷键：按下即激活对应风格包。默认空列表（issue #759）。 */
  stylePackHotkeys: StylePackHotkey[];
  /** Less Computer：是否启用。默认关闭。 */
  codingAgentEnabled: boolean;
  /** Agent 后端：claude-code-cli（默认）/ opencode-cli / codex-cli / dsh-cli。 */
  codingAgentProvider: CodingAgentProviderId;
  /**
   * Agent 模型，null = 交给后端自己的默认。
   * Claude 走别名（sonnet 等），OpenCode 要 `provider/model`，Codex 收裸模型名；
   * dsh 的 headless profile 没有模型开关，这一项对它无效。
   */
  codingAgentModel: string | null;
  /** 权限模式：plan/default/acceptEdits/bypassPermissions。 */
  codingAgentPermissionMode: CodingAgentPermissionMode;
  /** Agent 工作目录，null = 临时目录。 */
  codingAgentWorkdir: string | null;
  /** Agent 可执行文件路径/命令，null 或空 = 按后端取默认（claude / opencode）。 */
  codingAgentExe: string | null;
  /** Less Computer 按住说话快捷键。null = 停用；目前仅 macOS 显示/生效。 */
  codingAgentVoiceHotkey: ShortcutBinding | null;
  /** 热键 1：语音 Agent 面板键。null = 停用。 */
  codingAgentPanelHotkey: ShortcutBinding | null;
  /** 热键 2：快取用键（选中→Claude→回插）。null = 未配置。 */
  codingAgentQuickHotkey: ShortcutBinding | null;
  /** 本地 Qwen3-ASR 当前激活的模型 id。仅在 local-qwen3 系列 provider 时有意义。 */
  localAsrActiveModel: string;
  /** macOS 本地 Whisper 当前激活的模型 id。 */
  localWhisperActiveModel: string;
  /** 本地模型下载源镜像（'huggingface' / 'hf-mirror'）。 */
  localAsrMirror: string;
  /** 本地 ASR 引擎在内存中的保留时长（秒）。0 = 说完话即释放；
   *  300 = 默认 5 分钟；86400 ≈ 不释放（保持加载）。 */
  localAsrKeepLoadedSecs: number;
  /** Windows Foundry Local Whisper 当前激活的模型 alias。 */
  foundryLocalAsrModel: string;
  /** Windows Foundry Local native runtime 下载源。 */
  foundryLocalRuntimeSource: string;
  /** Windows Foundry Local Whisper 语言 hint。空字符串表示自动检测。 */
  foundryLocalAsrLanguageHint: string;
  /** Windows Foundry Local Whisper 模型在 runtime 中保持加载的秒数。 */
  foundryLocalAsrKeepLoadedSecs: number;
  /** Windows sherpa-onnx 本地 ASR 当前激活的模型 alias。 */
  sherpaOnnxModel: string;
  /** Windows sherpa-onnx 语言 hint。空字符串表示自动检测。 */
  sherpaOnnxLanguageHint: string;
  /** Windows sherpa-onnx 模型在 runtime 中保持加载的秒数。 */
  sherpaOnnxKeepLoadedSecs: number;
  /** 历史记录保留天数。0 = 不按时间清理（仍受 200 条上限）。默认 7。 */
  historyRetentionDays: number;
  /** 对话感知 polish 上下文窗口（分钟）。0 = 关闭。默认 5。详见 PR-A。 */
  polishContextWindowMinutes: number;
  /** 启动时静默运行（不弹主窗口）。Windows 开机自启场景常用——只想要后台 + 托盘，
   *  不想被主窗口打扰。开后所有启动路径都不弹窗，从菜单栏 / 托盘进入主窗口。默认 false。 */
  startMinimized: boolean;
  /** UI theme preference: follow OS, light, or dark. */
  themeMode: ThemeMode;
  /** 后台自动更新渠道。stable（默认）= AutoUpdateGate 查正式版 manifest；
   *  beta = 查 Beta manifest。About / Advanced 的手动检查按钮各自固定 stable/beta。 */
  updateChannel: UpdateChannel;
  /** 流式输入：润色 SSE 一边到达一边逐字模拟键盘事件输出到当前焦点。开启后用户感知到
   *  的处理时延显著降低。v1 限定 macOS + OpenAI-compatible provider，其他配置自动回落
   *  到原一次性插入。默认 true。 */
  streamingInsert: boolean;
  /** issue #440 一次性迁移标记：旧配置缺少该字段时后端会把老默认 false 迁到 true；
   *  迁移后用户再手动关掉 streamingInsert 时保留 false。 */
  streamingInsertDefaultMigrated: boolean;
  /** 流式输入成功后是否把最终润色文本写回剪贴板。开启后 Cmd+V 还能重复粘贴该次输出，
   *  与一次性路径行为对齐。默认 true。 */
  streamingInsertSaveClipboard: boolean;
  /** 是否把「用户正在写的那篇文档」中光标附近的原文送进 LLM 润色当上下文。
   *  默认 false —— 开启后每次听写都会读取前台 app 的正文并把其中一段发给 LLM 服务商。
   *  仅 macOS 有实现；密码框 / Secure Input / 密码管理器 / 终端一律硬拦。 */
  cursorContextEnabled: boolean;
  /** 总开关内的 LLM 上下文子开关。 */
  cursorContextLlmEnabled?: boolean;
  /** 总开关内的落字后手改学习子开关。 */
  editLearningEnabled?: boolean;
  /** 让未处理的即时建议进入可跨重启收件箱。 */
  vocabSuggestionInboxEnabled?: boolean;
  /** project 临时词条默认过期天数。 */
  temporaryVocabTtlDays?: number;
  temporaryVocabCapacity?: number;
  /** 概览页是否显示「年度活动」热力图卡。默认 true；关闭只隐藏卡片，活动计数照常记录。 */
  showOverviewActivityHeatmap: boolean;
  /** 易读布局：小屏或大字号时强制同行控件换行，避免横向溢出。默认 false。 */
  stackedRowLayout: boolean;
  /** 保守排版：除首页、顶栏、底栏与胶囊窗外，内容区强制单列满宽。默认 false。 */
  conservativeLayout: boolean;
  /** 主窗口启动 + 后台每 60 分钟自动检查更新。默认 true。
   *  Android：开启后自动检查并下载，校验后打开系统安装器。
   *  桌面：开启后自动检查，发现更新弹窗由用户确认安装。
   *  关闭后仅 Settings 手动「检查更新」按钮可用。 */
  autoUpdateCheck: boolean;
  /** 历史记录上限（条数）。null = 走默认 200；5..=200 之间为用户自定义。 */
  historyMaxEntries: number | null;
  /** 是否为每次会话保留原始麦克风音频文件（wav），用于排查 ASR 误识别 / 麦克风灵敏度。
   *  默认 false。开启后会占磁盘空间，受 historyRetentionDays 同样的清理策略约束。 */
  recordAudioForDebug: boolean;
  /** recordings/ 里保留的最近 wav 文件数。null = 跟随 200 硬上限；1..=200 之间为用户自定义。
   *  跟 historyMaxEntries 解耦——「文本档案多但 wav 只留最近 5 条」是合法组合。 */
  audioRecordingMaxEntries: number | null;
  /** Marketplace HTTP 基地址。空 = 本地开发默认 http://127.0.0.1:8090；生产填 https://api.<domain>。 */
  marketplaceBaseUrl: string;
  /** GitHub login 展示缓存。不用于认证；OAuth token 只存在 Rust CredentialsVault。 */
  marketplaceDevLogin: string;
  /** 是否启用远程输入（局域网手机录音）HTTPS+WS 服务。默认 false。 */
  remoteInputEnabled: boolean;
  /** 远程输入服务监听端口（HTTPS）。默认 8443。 */
  remoteInputPort: number;
  /** 远程输入配对码（6 位数字）。空 = server 首次启动时随机生成。 */
  remoteInputPin: string;
  /** 手机录音页默认交互方式：'toggle'（点击切换）/ 'hold'（按住说话）。 */
  remoteInputDefaultMode: 'toggle' | 'hold';
  /** Android: cross-app dictation insert strategy. */
  androidInsertStrategy: AndroidInsertStrategy;
  /** Android: floating overlay visibility trigger mode. */
  androidOverlayTrigger: AndroidOverlayTrigger;
  /** Android: how the floating overlay enters the armed interaction state. */
  androidOverlayActivationMode: AndroidOverlayActivationMode;
  /** Android: action performed by left swiping while the overlay is armed. */
  androidOverlayLeftSwipeAction: AndroidOverlayLeftSwipeAction;
  /** Android: vertical swipe direction that cancels recording. */
  androidOverlayCancelSwipeDirection: AndroidOverlayCancelSwipeDirection;
  /** Android: floating overlay control diameter in dp. */
  androidOverlaySizeDp: number;
}

export interface MarketplaceListItem {
  id: string;
  slug: string;
  name: string;
  description: string;
  authorLogin: string;
  version: string;
  baseMode: PolishMode;
  tags: string[];
  likeCount: number;
  downloadCount: number;
  publishedAt: string;
  updatedAt: string;
  /** 衍生关系：null = 原创；非空 = 衍生自 originPackId，UI 显「衍生自 @originAuthorLogin」。 */
  originPackId?: string | null;
  originAuthorLogin?: string | null;
}

export interface MarketplaceDetail extends MarketplaceListItem {
  prompt: string;
  state: 'pending' | 'approved' | 'rejected';
}

export interface MarketplaceMyPackItem extends MarketplaceListItem {
  state: 'pending' | 'approved' | 'rejected' | 'withdrawn' | 'superseded' | string;
}

export interface MicrophoneDevice {
  name: string;
  isDefault: boolean;
}

/** Rust 通过 `qa:state` 事件下发的 payload。
 *  v2 (issue #118 v2)：支持多轮对话，messages 数组每次由后端整段下发（单一可信源）。
 *  v2.1：开 `stream:true`，LLM 答案逐 chunk 通过 `answer_delta` 事件推前端边渲染。 */
export type QaStateKind =
  | 'idle'
  | 'recording'
  | 'loading'
  | 'thinking'
  | 'answer_delta'
  | 'answer'
  | 'error';

export interface QaChatMessage {
  role: 'user' | 'assistant';
  content: string;
  /** 未经模型安全信封转义的选区原文，仅用于 UI 文本展示。 */
  selectionText?: string;
}

export interface QaStatePayload {
  kind: QaStateKind;
  /** 后端会话 token；前端用它丢弃关闭/重开后迟到的旧轮事件。 */
  session_id?: string;
  /** 后端权威：当前已有的多轮对话历史（user → assistant 交替）。answer 事件带完整版。 */
  messages?: QaChatMessage[];
  /** recording 状态时附带的选区预览（前 60 字）。 */
  selection_preview?: string | null;
  /** error 状态时附带的提示。 */
  error?: string;
  /** answer_delta 事件时附带的本帧增量字符串。 */
  chunk?: string;
}

/**
 * Less Computer 语音 Agent 浮窗事件（窗口 label = "less-computer"，事件名
 * `less-computer:event`）。后端按 `kind` 标记，前端据此把交互渲染成聊天结构。
 */
export type LessComputerEvent = (
  /** 一轮用户气泡（语音指令转写）。fresh=true 表示新会话（清空历史）；否则追加为后续轮次。 */
  | { kind: 'user'; text: string; fresh?: boolean }
  /** Agent 启动，进入运行态。 */
  | { kind: 'started' }
  /** 流式回复增量（来自 CodingAgentEvent::Delta）。 */
  | { kind: 'delta'; text: string }
  /** 工具调用提示（来自 CodingAgentEvent::ToolUse，如 "Bash"）。 */
  | { kind: 'tool'; name: string }
  /** 会话上下文被压缩（来自 CodingAgentEvent::Compaction），输出流对应位置内嵌提示。 */
  | { kind: 'compaction' }
  /** 内联审批卡：高风险动作被护栏拦下，等用户 Approve / Deny。 */
  | { kind: 'approval'; token: string; command: string; reason: string }
  /** 运行完成：最终结果 + 成本（美元）。 */
  | { kind: 'completed'; text: string; costUsd?: number | null }
  /** 用户从胶囊取消正在运行的 Agent。 */
  | { kind: 'cancelled' }
  /** 运行出错。 */
  | { kind: 'error'; message: string }
) & {
  /** 单调事件序号（后端 emit 时编）。用于 less_computer_sync 重放与实时流去重；
   *  缓冲锁异常时后端可能省略，无 seq 的事件前端无条件应用。 */
  seq?: number;
};

/** 内置语言列表 — 前端 Settings UI 用，后端只接收原生名字符串拼 prompt。
 *  添加新语言时直接在这里加一项（原生名），无需修改后端。 */
export const SUPPORTED_LANGUAGES: readonly string[] = [
  '简体中文',
  '繁体中文',
  'English',
  '日本語',
  '한국어',
  'Français',
  'Deutsch',
  'Español',
  'Italiano',
  'Português',
  'Русский',
  'العربية',
  'Tiếng Việt',
  'ไทย',
  'हिन्दी',
] as const;

export type CapsuleState =
  | 'idle'
  | 'recording'
  | 'transcribing'
  | 'polishing'
  | 'done'
  | 'cancelled'
  | 'error';

/** 录音胶囊样式：'siri' = 流光 Siri 光效版（默认）；'classic' = Openless 经典药丸版。 */
export type CapsuleStyle = 'siri' | 'classic';

export interface CapsulePayload {
  state: CapsuleState;
  level: number; // 0..1 RMS
  elapsedMs: number;
  message: string | null;
  insertedChars: number | null;
  /** 当前 session 是否处于翻译模式（用户已按过 Shift）。详见 issue #4。 */
  translation: boolean;
  /** 当前是否是 Less Computer 会话：处理态文案显示 "using" 而非 "thinking"。 */
  operating?: boolean;
  /**
   * 预备态：胶囊已「乐观显示」（按下热键即弹出并播入场动画），但麦克风还没吐第一帧
   * PCM。为 true 时录音光条渲染成「待命」形态（柔和呼吸、不接真实电平），暗示用户稍候
   * 再开口；麦克风就绪后翻 false，光条点亮进入正式录音。只对 recording 有意义。
   */
  warming?: boolean;
  /**
   * 用户选择的胶囊样式（siri / classic）。随每次状态事件下发；缺失时回落默认
   * 'siri'，兼容旧后端 payload。
   */
  capsuleStyle?: CapsuleStyle;
  /**
   * 选区润色复用 capsule 的无焦点原生窗口，但渲染为轻量状态提示；缺失时保持原有
   * 语音/QA 胶囊行为，兼容旧后端 payload。
   */
  selectionPolish?: boolean;
  /** Esc 取消后可继续录音的历史/WAV session id；只在 3 秒恢复提示期间存在。 */
  recoverySessionId?: string | null;
}

export interface CredentialsStatus {
  activeAsrProvider: string;
  activeLlmProvider: string;
  /** 当前识别管线模式，前端据此渲染配置页与概览「已配置」判定。 */
  pipelineMode: PipelineMode;
  asrConfigured: boolean;
  llmConfigured: boolean;
  /** 多模态（omni）模型是否已配置。仅 multimodal 模式有意义。 */
  omniConfigured: boolean;
  /** 兼容旧字段（过渡期保留）。 */
  volcengineConfigured: boolean;
  arkConfigured: boolean;
}

export interface TodayMetrics {
  charsToday: number;
  segmentsToday: number;
  avgLatencyMs: number;
  totalDurationMs: number;
}

export type PermissionStatus =
  | 'granted'
  | 'denied'
  | 'notDetermined'
  | 'restricted'
  | 'notApplicable'
  | 'noDevice';

/** Runtime platform kind returned by `get_platform_capabilities`. */
export type PlatformKind = 'desktop' | 'android' | 'mobile';

/** Feature flags for desktop vs Android APK UI gating. Mirrors src-tauri PlatformCapabilities. */
export interface PlatformCapabilities {
  platform: PlatformKind;
  supportsDesktopHotkey: boolean;
  supportsTray: boolean;
  supportsOverlay: boolean;
  supportsImeInput: boolean;
  supportsLocalAsr: boolean;
  supportsLocalQwen3Mlx: boolean;
  supportsInAppDictation: boolean;
  supportsAutoUpdate: boolean;
}
