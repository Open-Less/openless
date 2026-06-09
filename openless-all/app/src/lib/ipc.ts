// ipc.ts — typed wrapper around Tauri `invoke`. When running outside Tauri
// (e.g. `vite dev` in a browser), every command falls back to mock data so
// the UI is still operable for visual review.

import type {
    ComboBinding,
    CorrectionRule,
    CredentialsStatus,
    DictationSession,
    DictionaryEntry,
    HotkeyCapability,
    MarketplaceDetail,
    MarketplaceListItem,
    MarketplaceMyPackItem,
    HotkeyStatus,
    MicrophoneDevice,
    PermissionStatus,
    PlatformCapabilities,
    CodingAgentPermissionMode,
    PolishMode,
    QaHotkeyBinding,
    ShortcutBinding,
    StylePack,
    StylePackExample,
    StylePackKind,
    StylePackRuntimeDiagnostics,
    StyleSystemPrompts,
    UpdateChannel,
    UserPreferences,
    VocabPresetStore,
    WindowsImeStatus,
    AndroidImeStatus,
    AndroidOverlayStatus,
    AndroidAccessibilityStatus,
    AndroidInsertStrategy,
    AndroidOverlayTrigger,
} from "./types"
export type { UpdateChannel, PlatformCapabilities } from "./types"
import { OL_DATA } from "./mockData"
import {
    defaultAppShortcutModifiers,
    defaultQaShortcut,
    formatComboLabel,
} from "./hotkey"
import {
    getPlatformCapabilities as loadPlatformCapabilities,
} from "./platform"

declare global {
    interface Window {
        __TAURI_INTERNALS__?: unknown
    }
}

const isTauri =
    globalThis.window !== undefined &&
    "__TAURI_INTERNALS__" in globalThis.window

let platformCapsPromise: Promise<PlatformCapabilities> | null = null

async function platformCapabilities(): Promise<PlatformCapabilities> {
    platformCapsPromise ??= loadPlatformCapabilities()
    return platformCapsPromise
}

export async function getPlatformCapabilities(): Promise<PlatformCapabilities> {
    return platformCapabilities()
}

export function getAndroidImeStatus(): Promise<AndroidImeStatus> {
    return invokeOrMock("get_android_ime_status", undefined, () => ({
        state: "notAndroid",
        enabled: false,
        selected: false,
        message: "Android IME is only available on Android",
    }))
}

export function getAndroidOverlayStatus(): Promise<AndroidOverlayStatus> {
    return invokeOrMock("get_android_overlay_status", undefined, () => ({
        permission: "notAndroid",
        overlayVisible: false,
        message: "Android overlay is only available on Android",
    }))
}

export function requestAndroidOverlayPermission(): Promise<{ launched: boolean; message: string }> {
    return invokeOrMock("request_android_overlay_permission", undefined, () => ({
        launched: false,
        message: "Mock: overlay permission unavailable in browser preview",
    }))
}

export function showAndroidOverlay(): Promise<void> {
    return invokeOrMock("show_android_overlay", undefined, () => undefined)
}

export function hideAndroidOverlay(): Promise<void> {
    return invokeOrMock("hide_android_overlay", undefined, () => undefined)
}

export function getAndroidAccessibilityStatus(): Promise<AndroidAccessibilityStatus> {
    return invokeOrMock("get_android_accessibility_status", undefined, () => ({
        state: "notAndroid",
        enabled: false,
        message: "Android accessibility is only available on Android",
    }))
}

export function requestAndroidAccessibilityPermission(): Promise<{ launched: boolean; message: string }> {
    return invokeOrMock("request_android_accessibility_permission", undefined, () => ({
        launched: false,
        message: "Mock: accessibility settings unavailable in browser preview",
    }))
}

export function requestAndroidImeSettings(): Promise<void> {
    return invokeOrMock("request_android_ime_settings", undefined, () => undefined)
}

export { isAndroid, isDesktop, isMobile } from "./platform"

const androidHotkeyCapability: HotkeyCapability = {
    adapter: "unavailable",
    availableTriggers: [],
    requiresAccessibilityPermission: false,
    supportsModifierOnlyTrigger: false,
    supportsSideSpecificModifiers: false,
    explicitFallbackAvailable: false,
    statusHint:
        "移动端不支持全局热键；请使用应用内录音按钮或悬浮窗（需授权）。",
}

const androidHotkeyStatus: HotkeyStatus = {
    adapter: "unavailable",
    state: "failed",
    message: "移动端不支持全局热键",
    lastError: {
        code: "unavailable",
        message: "Global hotkeys are not available on mobile",
    },
}

const androidWindowsImeStatus: WindowsImeStatus = {
    state: "notWindows",
    usingTsfBackend: false,
    message: "Not available on Android",
    dllPath: null,
}

export async function invokeOrMock<T>(
    cmd: string,
    args: Record<string, unknown> | undefined,
    mock: () => T,
): Promise<T> {
    if (!isTauri) {
        return mock()
    }
    const { invoke } = await import("@tauri-apps/api/core")
    return invoke<T>(cmd, args)
}

// ── Mock fixtures ──────────────────────────────────────────────────────
let mockSettings: UserPreferences = {
    hotkey: {
        trigger: "rightControl",
        mode: "toggle",
        keys: [{ code: "ControlRight" }],
    },
    dictationHotkey: { primary: "RightControl", modifiers: [] },
    defaultMode: "structured",
    enabledModes: ["raw", "light", "structured", "formal"],
    activeStylePackId: "builtin.structured",
    styleSystemPrompts: {
        raw: "只做最小化整理：补全标点、必要分句，保留原话顺序、用词和语气。",
        light: "把口语转写整理成自然文字，去掉口癖和重复，保留原意与语气。",
        structured: "把口述整理成结构清晰的文本，必要时按主题分组输出。",
        formal: "输出适合工作沟通与邮件场景的正式表达，不扩写事实。",
    },
    customStylePrompts: { raw: "", light: "", structured: "", formal: "" },
    launchAtLogin: false,
    showCapsule: true,
    muteDuringRecording: false,
    audioCueOnRecord: true,
    microphoneDeviceName: "",
    activeAsrProvider: "foundry-local-whisper",
    activeLlmProvider: "ark",
    llmThinkingEnabled: false,
    restoreClipboardAfterPaste: true,
    pasteShortcut: "ctrlV",
    allowNonTsfInsertionFallback: true,
    workingLanguages: ["简体中文"],
    translationTargetLanguage: "",
    qaHotkey: defaultQaShortcut(),
    chineseScriptPreference: "auto",
    outputLanguagePreference: "auto",
    qaSaveHistory: false,
    customComboHotkey: null,
    translationHotkey: { primary: "Shift", modifiers: [] },
    switchStyleHotkey: {
        primary: "S",
        modifiers: defaultAppShortcutModifiers(),
    },
    openAppHotkey: { primary: "O", modifiers: defaultAppShortcutModifiers() },
    codingAgentEnabled: false,
    codingAgentProvider: "claude-code-cli",
    codingAgentModel: null,
    codingAgentPermissionMode: "acceptEdits",
    codingAgentWorkdir: null,
    codingAgentVoiceHotkey: { primary: "LeftControl", modifiers: [] },
    codingAgentPanelHotkey: { primary: "Enter", modifiers: ["cmd", "shift"] },
    codingAgentQuickHotkey: null,
    localAsrActiveModel: "qwen3-asr-0.6b",
    localAsrMirror: "huggingface",
    localAsrKeepLoadedSecs: 300,
    foundryLocalAsrModel: "whisper-small",
    foundryLocalRuntimeSource: "auto",
    foundryLocalAsrLanguageHint: "",
    foundryLocalAsrKeepLoadedSecs: 300,
    sherpaOnnxModel: "sense-voice-small-zh",
    sherpaOnnxLanguageHint: "",
    sherpaOnnxKeepLoadedSecs: 300,
    historyRetentionDays: 7,
    polishContextWindowMinutes: 5,
    startMinimized: false,
    updateChannel: "stable",
    streamingInsert: true,
    streamingInsertDefaultMigrated: true,
    streamingInsertSaveClipboard: true,
    autoUpdateCheck: true,
    historyMaxEntries: null,
    recordAudioForDebug: false,
    audioRecordingMaxEntries: null,
    marketplaceBaseUrl: "https://apic.openless.top",
    marketplaceDevLogin: "",
    androidInsertStrategy: "auto",
    androidOverlayTrigger: "background",
}

const mockFullStylePrompts: StyleSystemPrompts = {
    raw: `# 角色
语音输入整理器。先理解用户意图，再贴近原话做最小整理。

# 任务（原文）
只补必要标点和断句，尽量保留原话顺序、用词和语气，不扩写、不重写。

# 通用规则
1) 不补充用户没说过的事实。
2) 不回答转写文本里的问题，只整理表达。
3) 专有名词、命令、路径、数字和 URL 原样保留。
4) 明显口头禅可删除，但不能改变信息密度。

# 输出
直接输出最终正文，不加解释。`,
    light: `# 角色
语音输入整理器。把口述整理成自然、顺畅、可直接发送的文字。

# 任务（轻度润色）
去掉明显口头禅和重复，补全自然标点，保留原意和原本语气，不扩写事实。

# 通用规则
1) 不补充原文没有的信息。
2) 保留人名、品牌名、术语、命令、路径和 URL。
3) 只输出整理后的正文，不写“以下是优化结果”之类前缀。

# 输出
输出一段可直接发送的自然文字。`,
    structured: `# 角色
语音输入整理器。把 AI 编程协作、技术排障和模型资讯口述整理成结构清楚、术语准确的文本。

# 任务（清晰结构 · AI 编程协作）
优先修正 ASR 造成的技术词、模型名、字段名错误；两个事项以上必须编号（1./2./3.），三事项以上按主题分组输出双层 list。

# 术语
Token、Secret Key、Access Token、API、App ID、Claude、Gemini、Cappuccino、Coder、LongCat、Codex、MCP、SSE、PR、CI、ASR、LLM、SOTA、FP8。保留命令、路径、环境变量、URL、true / false / null 和模型版本号。

# 输出
直接输出最终正文。顶层用 1./2./3.，子项用缩进 3 个空格的 (a)(b)(c)。不加解释。`,
    formal: `# 角色
语音输入整理器。把口述整理成适合邮件、同步和正式沟通的专业表达。

# 任务（正式表达）
补足句式与标点，让表达更完整、克制、专业，但不添加空泛客套，也不擅自扩写事实。

# 通用规则
1) 不承诺用户没说过的内容。
2) 保留专有名词、数字、时间、路径和术语。
3) 只输出最终正文，不附带解释或 markdown 围栏。

# 输出
输出可直接发送的正式文本。`,
}

mockSettings = {
    ...mockSettings,
    styleSystemPrompts: mockFullStylePrompts,
    workingLanguages: ["简体中文"],
}

const mockDefaultStyleSystemPrompts: StyleSystemPrompts = {
    ...mockSettings.styleSystemPrompts,
}

const mockBuiltinExamples: Record<PolishMode, StylePackExample[]> = {
    raw: [
        {
            title: "最小整理",
            input: "今天下午那个会先别取消我晚点再确认一下然后把下周二也先空出来",
            output: "今天下午那个会先别取消，我晚点再确认一下。然后把下周二也先空出来。",
        },
    ],
    light: [
        {
            title: "聊天消息",
            input: "你帮我跟设计那边说一下这个首页先别上线我晚上再过一遍",
            output: "你帮我跟设计那边说一下，这个首页先别上线，我今晚再过一遍。",
        },
    ],
    structured: [
        {
            title: "AI 编程任务",
            input: "帮我给 codex 提个任务先把登录页 bug 修掉然后补一下 README 里面的环境变量说明还有那个西克瑞特 key 别写死到代码里",
            output: "帮忙给 Codex 提个任务，主要包含以下内容：\n\n1. 登录页修复\n   (a) 修复登录页相关 bug。\n2. 文档与配置\n   (a) 补充 README 中的环境变量说明。\n   (b) 确认 Secret Key 不被硬编码到代码里。",
        },
    ],
    formal: [
        {
            title: "工作同步",
            input: "你帮我发个消息说这个需求今天先不上了等测试和产品都确认完我们再一起推进",
            output: "麻烦帮我同步一下：这个需求今天先不上线，待测试和产品都确认完成后，我们再统一推进。",
        },
    ],
}

function makeMockStylePack(
    id: string,
    kind: StylePackKind,
    baseMode: PolishMode,
    name: string,
    description: string,
    prompt: string,
    tags: string[],
): StylePack {
    return {
        id,
        name,
        description,
        author: "OpenLess",
        version: "1.0.0",
        kind,
        baseMode,
        prompt,
        examples: mockBuiltinExamples[baseMode].map((example) => ({
            ...example,
        })),
        tags,
        iconPath: null,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        enabled: true,
        active: false,
        recommendedModel: null,
        compatibleAppVersion: "1.0.0",
    }
}

let mockStylePacks: StylePack[] = [
    makeMockStylePack(
        "builtin.raw",
        "builtin",
        "raw",
        "原文",
        "尽量保留原话顺序和语气，只做必要的断句与标点整理。",
        mockSettings.styleSystemPrompts.raw,
        ["原文", "最小改写"],
    ),
    makeMockStylePack(
        "builtin.light",
        "builtin",
        "light",
        "轻度润色",
        "把口述整理成顺畅、自然、可直接发送的文字，不扩写事实。",
        mockSettings.styleSystemPrompts.light,
        ["沟通", "自然"],
    ),
    makeMockStylePack(
        "builtin.structured",
        "builtin",
        "structured",
        "清晰结构",
        "适合多事项和多主题口述，自动整理为层次清楚的结构化输出。",
        mockSettings.styleSystemPrompts.structured,
        ["结构化", "条理"],
    ),
    makeMockStylePack(
        "builtin.formal",
        "builtin",
        "formal",
        "正式表达",
        "适合邮件、同步和工作沟通场景，语气更完整、专业、克制。",
        mockSettings.styleSystemPrompts.formal,
        ["正式", "工作沟通"],
    ),
    {
        ...makeMockStylePack(
            "imported.creator-note",
            "imported",
            "light",
            "创作者口播",
            "给短视频口播和社区帖文使用，句子更紧凑，保留情绪和节奏。",
            "你是一个负责整理创作者口播稿的编辑。请把输入整理成适合发帖和口播的自然文本，保留节奏感，不要补充原文没有的信息。",
            ["社区", "口播", "节奏感"],
        ),
        author: "Demo Community",
    },
]

function cloneStylePack(stylePack: StylePack): StylePack {
    return {
        ...stylePack,
        tags: [...stylePack.tags],
        examples: stylePack.examples.map((example) => ({ ...example })),
    }
}

function cloneMockStylePacks(): StylePack[] {
    return mockStylePacks.map(cloneStylePack)
}

function composeMockStylePackRuntimeDiagnostics(
    stylePack: StylePack,
): StylePackRuntimeDiagnostics {
    const trimmedPrompt = stylePack.prompt.trimEnd()
    const contextPremise = mockSettings.workingLanguages.length
        ? [
              "# Context",
              `Working languages: ${mockSettings.workingLanguages.join(", ")}`,
          ].join("\n")
        : ""
    const hotwordLines = [`GitHub`, `OpenLess`]
    const hotwordBlock =
        hotwordLines.length > 0
            ? [
                  "Hotwords (keep the spelling below when they appear in the transcript):",
                  ...hotwordLines.map((word) => `- ${word}`),
              ].join("\n")
            : ""
    const singleTurnPrompt = [contextPremise, trimmedPrompt, hotwordBlock]
        .filter(Boolean)
        .join("\n\n")
    const historyInstruction =
        "When prior turns exist, do not repeat previous assistant outputs. Only polish the current transcript."
    const multiTurnPrompt = `${singleTurnPrompt}\n\n${historyInstruction}`
    return {
        packId: stylePack.id,
        packName: stylePack.name,
        packPrompt: stylePack.prompt,
        packPromptChars: stylePack.prompt.length,
        contextPremise,
        contextPremiseChars: contextPremise.length,
        hotwordBlock,
        hotwordBlockChars: hotwordBlock.length,
        historyInstruction,
        historyInstructionChars: historyInstruction.length,
        singleTurnPrompt,
        singleTurnPromptChars: singleTurnPrompt.length,
        multiTurnPrompt,
        multiTurnPromptChars: multiTurnPrompt.length,
        workingLanguages: [...mockSettings.workingLanguages],
        hotwords: [...hotwordLines],
        contextWindowMinutes: mockSettings.polishContextWindowMinutes,
        includesContextPremise: Boolean(contextPremise),
        includesHotwordBlock: hotwordLines.length > 0,
        includesHistoryInstruction: true,
        previewOmitsFrontApp: true,
    }
}

function syncMockSettingsFromStylePacks() {
    const enabled = mockStylePacks.filter((pack) => pack.enabled)
    const active =
        mockStylePacks.find(
            (pack) =>
                pack.id === mockSettings.activeStylePackId && pack.enabled,
        ) ??
        enabled[0] ??
        mockStylePacks[0]
    mockStylePacks = mockStylePacks.map((pack) => ({
        ...pack,
        active: pack.id === active.id,
    }))
    mockSettings = {
        ...mockSettings,
        activeStylePackId: active.id,
        defaultMode: active.baseMode,
        enabledModes: ["raw", "light", "structured", "formal"].filter((mode) =>
            mockStylePacks.some(
                (pack) => pack.enabled && pack.baseMode === mode,
            ),
        ) as PolishMode[],
        styleSystemPrompts: {
            raw:
                mockStylePacks.find((pack) => pack.id === "builtin.raw")
                    ?.prompt ?? mockSettings.styleSystemPrompts.raw,
            light:
                mockStylePacks.find((pack) => pack.id === "builtin.light")
                    ?.prompt ?? mockSettings.styleSystemPrompts.light,
            structured:
                mockStylePacks.find((pack) => pack.id === "builtin.structured")
                    ?.prompt ?? mockSettings.styleSystemPrompts.structured,
            formal:
                mockStylePacks.find((pack) => pack.id === "builtin.formal")
                    ?.prompt ?? mockSettings.styleSystemPrompts.formal,
        },
    }
}

syncMockSettingsFromStylePacks()

const mockHotkeyCapability: HotkeyCapability = {
    adapter: "windowsLowLevel",
    availableTriggers: [
        "rightControl",
        "rightAlt",
        "leftControl",
        "rightCommand",
        "custom",
    ],
    requiresAccessibilityPermission: false,
    supportsModifierOnlyTrigger: true,
    supportsSideSpecificModifiers: true,
    explicitFallbackAvailable: false,
    statusHint:
        "默认建议使用“右Ctrl + 单击”；若更习惯按住说话，可在录音设置里切回“按住”。若无响应，可在权限页查看 hook 安装状态。",
}

const mockCredentialsStatus: CredentialsStatus = {
    activeAsrProvider: "foundry-local-whisper",
    activeLlmProvider: "ark",
    asrConfigured: true,
    llmConfigured: true,
    volcengineConfigured: true,
    arkConfigured: true,
}

export interface ProviderCheckResult {
    ok: boolean
}

export interface ProviderModelsResult {
    models: string[]
}

const mockHotkeyStatus: HotkeyStatus = {
    adapter: "windowsLowLevel",
    state: "installed",
    message: "Windows 低层键盘 hook 已安装",
    lastError: null,
}

const mockWindowsImeStatus: WindowsImeStatus = {
    state: "notWindows",
    usingTsfBackend: false,
    message: "Browser dev mock",
    dllPath: null,
}

const mockMicrophoneDevices: MicrophoneDevice[] = [
    { name: "Built-in Microphone", isDefault: true },
    { name: "USB Microphone", isDefault: false },
]

const mockHistory: DictationSession[] = OL_DATA.history.map((h, i) => ({
    id: `mock-${i}`,
    createdAt: new Date().toISOString(),
    rawTranscript: h.preview,
    finalText: h.preview,
    mode: "structured",
    stylePackId: "builtin.structured",
    translationActive: false,
    polishSource: null,
    appBundleId: null,
    appName: "VS Code",
    insertStatus: "inserted",
    errorCode: null,
    durationMs: 600,
    dictionaryEntryCount: 28,
    hasAudioRecording: null,
}))

const mockVocab: DictionaryEntry[] = OL_DATA.vocab.map((v, i) => ({
    id: `vocab-${i}`,
    phrase: v.word,
    note: null,
    enabled: true,
    hits: v.count,
    createdAt: new Date().toISOString(),
}))

const mockCorrectionRules: CorrectionRule[] = [
    {
        id: "rule-quantity-classifier",
        pattern: "{num}粒",
        replacement: "{num}例",
        enabled: true,
        createdAt: new Date().toISOString(),
    },
]

// ── Settings ───────────────────────────────────────────────────────────
export function getSettings(): Promise<UserPreferences> {
    return invokeOrMock("get_settings", undefined, () => ({ ...mockSettings }))
}

export function getDefaultStyleSystemPrompts(): Promise<StyleSystemPrompts> {
    return invokeOrMock("get_default_style_system_prompts", undefined, () => ({
        ...mockDefaultStyleSystemPrompts,
    }))
}

export function setSettings(prefs: UserPreferences): Promise<void> {
    return invokeOrMock("set_settings", { prefs }, () => {
        mockSettings = { ...prefs }
        mockStylePacks = mockStylePacks.map((pack) => {
            if (pack.kind === "builtin") {
                return {
                    ...pack,
                    enabled: prefs.enabledModes.includes(pack.baseMode),
                    prompt: prefs.styleSystemPrompts[pack.baseMode],
                }
            }
            return { ...pack }
        })
        syncMockSettingsFromStylePacks()
        return undefined
    })
}

// ── Release channel (Beta opt-in) ──────────────────────────────────────
// 渠道偏好与 fetch_latest_beta_release 实际效果只在 Tauri runtime 内有意义；
// 浏览器开发模式下走 mock，避免设置页因 invoke 抛错而白屏。
// UpdateChannel 类型搬到 types.ts（UserPreferences.updateChannel 字段使用），
// 这里 re-export 保持外部模块（SettingsModal 等）import 路径不变。

export interface LatestBetaRelease {
    tagName: string
    htmlUrl: string
    publishedAt: string
}

export interface AppUpdateMetadata {
    rid: number
    currentVersion: string
    version: string
    date?: string | null
    body?: string | null
    rawJson: Record<string, unknown>
}

export function getUpdateChannel(): Promise<UpdateChannel> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return "stable" as UpdateChannel
        }
        return invokeOrMock(
            "get_update_channel",
            undefined,
            () => "stable" as UpdateChannel,
        )
    })
}

export function setUpdateChannel(channel: UpdateChannel): Promise<void> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return undefined
        }
        return invokeOrMock("set_update_channel", { channel }, () => undefined)
    })
}

export function fetchLatestBetaRelease(): Promise<LatestBetaRelease | null> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return null
        }
        return invokeOrMock("fetch_latest_beta_release", undefined, () => null)
    })
}

export function appCheckUpdateWithChannel(
    timeoutMs: number,
    channel?: UpdateChannel | null,
): Promise<AppUpdateMetadata | null> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return null
        }
        return invokeOrMock(
            "app_check_update_with_channel",
            { timeoutMs, channel: channel ?? null },
            () => null,
        )
    })
}

export function getHotkeyStatus(): Promise<HotkeyStatus> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsDesktopHotkey) {
            return androidHotkeyStatus
        }
        return invokeOrMock("get_hotkey_status", undefined, () => mockHotkeyStatus)
    })
}

export function getHotkeyCapability(): Promise<HotkeyCapability> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsDesktopHotkey) {
            return androidHotkeyCapability
        }
        return invokeOrMock(
            "get_hotkey_capability",
            undefined,
            () => mockHotkeyCapability,
        )
    })
}

export function getWindowsImeStatus(): Promise<WindowsImeStatus> {
    return platformCapabilities().then((caps) => {
        if (caps.platform === "android") {
            return androidWindowsImeStatus
        }
        return invokeOrMock(
            "get_windows_ime_status",
            undefined,
            () => mockWindowsImeStatus,
        )
    })
}

export interface NetworkCheckResult {
  online: boolean;
  latencyMs: number | null;
}

export function checkNetwork(): Promise<NetworkCheckResult> {
  return invokeOrMock<NetworkCheckResult>('check_network', undefined, () => ({
    online: true,
    latencyMs: 42,
  }));
}

export function listMicrophoneDevices(): Promise<MicrophoneDevice[]> {
    return invokeOrMock(
        "list_microphone_devices",
        undefined,
        () => mockMicrophoneDevices,
    )
}

export function startMicrophoneLevelMonitor(deviceName: string): Promise<void> {
    return invokeOrMock(
        "start_microphone_level_monitor",
        { deviceName },
        () => undefined,
    )
}

export function stopMicrophoneLevelMonitor(): Promise<void> {
    return invokeOrMock(
        "stop_microphone_level_monitor",
        undefined,
        () => undefined,
    )
}

export function isWaylandCliMode(): Promise<boolean> {
    return invokeOrMock("is_wayland_cli_mode", undefined, () => false)
}

// ── Credentials ────────────────────────────────────────────────────────
export function getCredentials(): Promise<CredentialsStatus> {
    return invokeOrMock(
        "get_credentials",
        undefined,
        () => mockCredentialsStatus,
    )
}

export function setCredential(account: string, value: string): Promise<void> {
    return invokeOrMock("set_credential", { account, value }, () => undefined)
}

export function setActiveAsrProvider(provider: string): Promise<void> {
    return invokeOrMock(
        "set_active_asr_provider",
        { provider },
        () => undefined,
    )
}

export function setActiveLlmProvider(provider: string): Promise<void> {
    return invokeOrMock(
        "set_active_llm_provider",
        { provider },
        () => undefined,
    )
}

export function readCredential(account: string): Promise<string | null> {
    return invokeOrMock<string | null>(
        "read_credential",
        { account },
        () => null,
    )
}

export function validateProviderCredentials(
    kind: "llm" | "asr",
): Promise<ProviderCheckResult> {
    return invokeOrMock("validate_provider_credentials", { kind }, () => ({
        ok: true,
    }))
}

export function listProviderModels(
    kind: "llm" | "asr",
): Promise<ProviderModelsResult> {
    return invokeOrMock("list_provider_models", { kind }, () => ({
        models:
            kind === "llm"
                ? ["gpt-4o", "deepseek-v4-flash", "deepseek-v4-pro"]
                : ["whisper-1"],
    }))
}

// ── History ────────────────────────────────────────────────────────────
export function listHistory(): Promise<DictationSession[]> {
    return invokeOrMock("list_history", undefined, () => mockHistory)
}

export function deleteHistoryEntry(id: string): Promise<void> {
    return invokeOrMock("delete_history_entry", { id }, () => undefined)
}

export function clearHistory(): Promise<void> {
    return invokeOrMock("clear_history", undefined, () => undefined)
}

/** 读取某次会话的原始麦克风 wav 字节流。仅当 prefs.recordAudioForDebug 当时打开
 *  并且文件没被 retention 清理掉时才有内容；其他情况后端会返回 "recording not found" 错。
 *  调用方应仅在 session.hasAudioRecording === true 时触发，避免无效 IPC。 */
export function readAudioRecording(sessionId: string): Promise<Uint8Array> {
    return invokeOrMock(
        "read_audio_recording",
        { sessionId },
        () => new Uint8Array(),
    ).then((value) => {
        // Tauri 默认把 Vec<u8> 序列化为 number[]，前端拿到的是普通数组；统一转 Uint8Array。
        if (value instanceof Uint8Array) return value
        if (Array.isArray(value)) return new Uint8Array(value as number[])
        return new Uint8Array(value as ArrayBuffer)
    })
}

// ── Vocab ──────────────────────────────────────────────────────────────
export function listVocab(): Promise<DictionaryEntry[]> {
    return invokeOrMock("list_vocab", undefined, () => mockVocab)
}

export function addVocab(
    phrase: string,
    note?: string,
): Promise<DictionaryEntry> {
    return invokeOrMock("add_vocab", { phrase, note }, () => ({
        id: `vocab-new-${Date.now()}`,
        phrase,
        note: note ?? null,
        enabled: true,
        hits: 0,
        createdAt: new Date().toISOString(),
    }))
}

export function removeVocab(id: string): Promise<void> {
    return invokeOrMock("remove_vocab", { id }, () => undefined)
}

export function setVocabEnabled(id: string, enabled: boolean): Promise<void> {
    return invokeOrMock("set_vocab_enabled", { id, enabled }, () => undefined)
}

export function listCorrectionRules(): Promise<CorrectionRule[]> {
    return invokeOrMock(
        "list_correction_rules",
        undefined,
        () => mockCorrectionRules,
    )
}

export function addCorrectionRule(
    pattern: string,
    replacement: string,
): Promise<CorrectionRule> {
    return invokeOrMock(
        "add_correction_rule",
        { pattern, replacement },
        () => ({
            id: `rule-new-${Date.now()}`,
            pattern,
            replacement,
            enabled: true,
            createdAt: new Date().toISOString(),
        }),
    )
}

export function removeCorrectionRule(id: string): Promise<void> {
    return invokeOrMock("remove_correction_rule", { id }, () => undefined)
}

export function setCorrectionRuleEnabled(
    id: string,
    enabled: boolean,
): Promise<void> {
    return invokeOrMock(
        "set_correction_rule_enabled",
        { id, enabled },
        () => undefined,
    )
}

export function listVocabPresets(): Promise<VocabPresetStore> {
    return invokeOrMock("list_vocab_presets", undefined, () => ({
        custom: [],
        overrides: [],
        disabledBuiltinPresetIds: [],
    }))
}

export function saveVocabPresets(store: VocabPresetStore): Promise<void> {
    return invokeOrMock("save_vocab_presets", { store }, () => undefined)
}

// ── Dictation lifecycle ────────────────────────────────────────────────
export function startDictation(): Promise<void> {
    return invokeOrMock("start_dictation", undefined, () => undefined)
}

export function stopDictation(): Promise<void> {
    return invokeOrMock("stop_dictation", undefined, () => undefined)
}

export function cancelDictation(): Promise<void> {
    return invokeOrMock("cancel_dictation", undefined, () => undefined)
}

export function handleWindowHotkeyEvent(
    eventType: "keydown" | "keyup",
    key: string,
    code: string,
    repeat: boolean,
): Promise<void> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsDesktopHotkey) {
            return undefined
        }
        return invokeOrMock(
            "handle_window_hotkey_event",
            { event_type: eventType, key, code, repeat },
            () => undefined,
        )
    })
}

// ── Polish ─────────────────────────────────────────────────────────────
export function repolish(rawText: string, mode: PolishMode): Promise<string> {
    return invokeOrMock("repolish", { rawText, mode }, () => rawText)
}

export function setDefaultPolishMode(mode: PolishMode): Promise<void> {
    return invokeOrMock("set_default_polish_mode", { mode }, () => {
        const packId = `builtin.${mode}`
        mockStylePacks = mockStylePacks.map((pack) => ({
            ...pack,
            enabled: pack.id === packId ? true : pack.enabled,
            active: pack.id === packId,
        }))
        mockSettings = { ...mockSettings, activeStylePackId: packId }
        syncMockSettingsFromStylePacks()
        return undefined
    })
}

export function setStyleEnabled(
    mode: PolishMode,
    enabled: boolean,
): Promise<void> {
    return invokeOrMock("set_style_enabled", { mode, enabled }, () => {
        const packId = `builtin.${mode}`
        mockStylePacks = mockStylePacks.map((pack) =>
            pack.id === packId ? { ...pack, enabled } : { ...pack },
        )
        syncMockSettingsFromStylePacks()
        return undefined
    })
}

export function listStylePacks(): Promise<StylePack[]> {
    return invokeOrMock("list_style_packs", undefined, () =>
        cloneMockStylePacks(),
    )
}

export function saveStylePack(stylePack: StylePack): Promise<StylePack> {
    return invokeOrMock("save_style_pack", { stylePack }, () => {
        mockStylePacks = mockStylePacks.map((pack) =>
            pack.id === stylePack.id ? cloneStylePack(stylePack) : pack,
        )
        syncMockSettingsFromStylePacks()
        return cloneStylePack(
            mockStylePacks.find((pack) => pack.id === stylePack.id) ??
                stylePack,
        )
    })
}

export function createStylePackFromTemplate(
    template: StylePack,
): Promise<StylePack> {
    return invokeOrMock("create_style_pack_from_template", { template }, () => {
        const created: StylePack = {
            ...cloneStylePack(template),
            id: `imported-mock-${Date.now()}`,
            kind: "imported",
            active: false,
            enabled: true,
            createdAt: new Date().toISOString(),
            updatedAt: new Date().toISOString(),
        }
        mockStylePacks = [...mockStylePacks, created]
        return cloneStylePack(created)
    })
}

export function previewStylePackRuntime(
    stylePack: StylePack,
): Promise<StylePackRuntimeDiagnostics> {
    return invokeOrMock("preview_style_pack_runtime", { stylePack }, () =>
        composeMockStylePackRuntimeDiagnostics(stylePack),
    )
}

export function setActiveStylePack(id: string): Promise<StylePack> {
    return invokeOrMock("set_active_style_pack", { id }, () => {
        mockStylePacks = mockStylePacks.map((pack) => ({
            ...pack,
            enabled: pack.id === id ? true : pack.enabled,
            active: pack.id === id,
        }))
        mockSettings = { ...mockSettings, activeStylePackId: id }
        syncMockSettingsFromStylePacks()
        return cloneStylePack(mockStylePacks.find((pack) => pack.id === id)!)
    })
}

export function setStylePackEnabled(
    id: string,
    enabled: boolean,
): Promise<StylePack[]> {
    return invokeOrMock("set_style_pack_enabled", { id, enabled }, () => {
        mockStylePacks = mockStylePacks.map((pack) =>
            pack.id === id ? { ...pack, enabled } : { ...pack },
        )
        syncMockSettingsFromStylePacks()
        return cloneMockStylePacks()
    })
}

export function resetBuiltinStylePack(id: string): Promise<StylePack> {
    return invokeOrMock("reset_builtin_style_pack", { id }, () => {
        const builtinDefaults: Record<string, StylePack> = {
            "builtin.raw": makeMockStylePack(
                "builtin.raw",
                "builtin",
                "raw",
                "原文",
                "尽量保留原话顺序和语气，只做必要的断句与标点整理。",
                mockDefaultStyleSystemPrompts.raw,
                ["原文", "最小改写"],
            ),
            "builtin.light": makeMockStylePack(
                "builtin.light",
                "builtin",
                "light",
                "轻度润色",
                "把口述整理成顺畅、自然、可直接发送的文字，不扩写事实。",
                "把口述整理成自然、顺畅、可直接发送的文字，去掉口头禅和重复，保留原意与语气。",
                ["沟通", "自然"],
            ),
            "builtin.structured": makeMockStylePack(
                "builtin.structured",
                "builtin",
                "structured",
                "清晰结构",
                "面向 AI 编程协作、技术排障和模型资讯，优先保证术语与结构准确。",
                mockDefaultStyleSystemPrompts.structured,
                ["AI 编程", "技术结构化"],
            ),
            "builtin.formal": makeMockStylePack(
                "builtin.formal",
                "builtin",
                "formal",
                "正式表达",
                "适合邮件、同步和工作沟通场景，语气更完整、专业、克制。",
                "输出适合工作沟通、邮件和汇报场景的正式表达，不扩写事实。",
                ["正式", "工作沟通"],
            ),
        }
        const current = mockStylePacks.find((pack) => pack.id === id)
        const reset = builtinDefaults[id]
        if (!current || !reset) {
            throw new Error(`style pack not found: ${id}`)
        }
        mockStylePacks = mockStylePacks.map((pack) =>
            pack.id === id
                ? {
                      ...reset,
                      enabled: current.enabled,
                      active: current.active,
                  }
                : pack,
        )
        syncMockSettingsFromStylePacks()
        return cloneStylePack(mockStylePacks.find((pack) => pack.id === id)!)
    })
}

export function deleteStylePack(id: string): Promise<void> {
    return invokeOrMock("delete_style_pack", { id }, () => {
        mockStylePacks = mockStylePacks.filter((pack) => pack.id !== id)
        syncMockSettingsFromStylePacks()
        return undefined
    })
}

export function importStylePackFromZip(zipPath: string): Promise<StylePack> {
    return invokeOrMock("import_style_pack_from_zip", { zipPath }, () => {
        const seed = Date.now()
        const pack = {
            ...makeMockStylePack(
                `imported.mock-${seed}`,
                "imported",
                "light",
                "导入风格包",
                `从 ${zipPath.split(/[\\\\/]/).pop() || "ZIP"} 导入的风格包`,
                "你是一个负责把口述整理成清晰、利落、适合社区分享文本的编辑，请完整保留事实，不要补充原文没有的信息。",
                ["导入", "ZIP"],
            ),
            author: "Imported ZIP",
        }
        mockStylePacks = [pack, ...mockStylePacks]
        syncMockSettingsFromStylePacks()
        return cloneStylePack(pack)
    })
}

export function exportStylePackToZip(
    id: string,
    targetPath: string,
): Promise<string> {
    return invokeOrMock(
        "export_style_pack_to_zip",
        { id, targetPath },
        () => targetPath,
    )
}

// ── Permissions ────────────────────────────────────────────────────────
export function checkAccessibilityPermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "check_accessibility_permission",
        undefined,
        () => "granted" as const,
    )
}

export function requestAccessibilityPermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "request_accessibility_permission",
        undefined,
        () => "granted" as const,
    )
}

export function checkMicrophonePermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "check_microphone_permission",
        undefined,
        () => "granted" as const,
    )
}

export function requestMicrophonePermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "request_microphone_permission",
        undefined,
        () => "granted" as const,
    )
}

export function openSystemSettings(
    pane: "accessibility" | "microphone",
): Promise<void> {
    return invokeOrMock("open_system_settings", { pane }, () => undefined)
}

export function triggerMicrophonePrompt(): Promise<void> {
    return invokeOrMock("trigger_microphone_prompt", undefined, () => undefined)
}

export function restartApp(): Promise<void> {
    return invokeOrMock("restart_app", undefined, () => undefined)
}

// ── QA (划词语音问答) ───────────────────────────────────────────────────
// 详见 issue #118。后端会发 `qa:state` / `qa:dismiss` 事件；前端通过下面四个
// 命令查询与控制 QA 浮窗。
export function getQaHotkeyLabel(): Promise<string> {
    return invokeOrMock("get_qa_hotkey_label", undefined, () =>
        formatComboLabel(defaultQaShortcut()),
    )
}

export function setQaHotkey(binding: QaHotkeyBinding | null): Promise<void> {
    return invokeOrMock("set_qa_hotkey", { binding }, () => undefined)
}

export function qaWindowDismiss(): Promise<void> {
    return invokeOrMock("qa_window_dismiss", undefined, () => undefined)
}

export function qaWindowPin(pinned: boolean): Promise<void> {
    return invokeOrMock("qa_window_pin", { pinned }, () => undefined)
}

// ── Less Computer 浮窗 ────────────────────────────────────────────────
/** 用户点 ✕ / 按 Esc 关闭 Less Computer 浮窗（隐藏窗口）。 */
export function lessComputerWindowDismiss(): Promise<void> {
    return invokeOrMock("less_computer_window_dismiss", undefined, () => undefined)
}

/** 内联审批卡的 Approve / Deny 回执。token 关联到等待中的拦截动作。 */
export function lessComputerApprove(
    token: string,
    approved: boolean,
): Promise<void> {
    return invokeOrMock(
        "less_computer_approve",
        { token, approved },
        () => undefined,
    )
}

/** 前端按内容测高后回传，后端 clamp + bottom-anchored 重新摆放浮窗。 */
export function lessComputerWindowResize(height: number): Promise<void> {
    return invokeOrMock(
        "less_computer_window_resize",
        { height },
        () => undefined,
    )
}

// ── Combo Hotkey (自定义录音组合键) ───────────────────────────────────
export function validateComboHotkey(binding: ComboBinding): Promise<void> {
    return invokeOrMock("validate_combo_hotkey", { binding }, () => undefined)
}

export function setComboHotkey(binding: ComboBinding): Promise<void> {
    return invokeOrMock("set_combo_hotkey", { binding }, () => undefined)
}

export function validateShortcutBinding(
    binding: ShortcutBinding,
): Promise<void> {
    return invokeOrMock(
        "validate_shortcut_binding",
        { binding },
        () => undefined,
    )
}

export function setDictationHotkey(binding: ShortcutBinding): Promise<void> {
    return invokeOrMock("set_dictation_hotkey", { binding }, () => undefined)
}

export function setTranslationHotkey(binding: ShortcutBinding): Promise<void> {
    return invokeOrMock("set_translation_hotkey", { binding }, () => undefined)
}

// binding = null 表示停用（清空全局键），与 set_qa_hotkey 一致（issue #576）。
export function setSwitchStyleHotkey(binding: ShortcutBinding | null): Promise<void> {
    return invokeOrMock("set_switch_style_hotkey", { binding }, () => undefined)
}

export function setOpenAppHotkey(binding: ShortcutBinding | null): Promise<void> {
    return invokeOrMock("set_open_app_hotkey", { binding }, () => undefined)
}

export function setShortcutRecordingActive(active: boolean): Promise<void> {
    return invokeOrMock(
        "set_shortcut_recording_active",
        { active },
        () => undefined,
    )
}

export async function openExternal(url: string): Promise<void> {
    if (!isTauri) {
        window.open(url, "_blank", "noopener,noreferrer")
        return
    }
    try {
        const { open } = await import("@tauri-apps/plugin-shell")
        await open(url)
        return
    } catch (error) {
        console.warn("[external-open] shell plugin failed", error)
    }
    try {
        const { invoke } = await import("@tauri-apps/api/core")
        await invoke("open_external_url", { url })
        return
    } catch (error) {
        console.warn("[external-open] native fallback failed", error)
    }
    window.open(url, "_blank", "noopener,noreferrer")
}

/**
 * 让用户选 save 路径并把当前会话日志（openless.log）复制过去。
 * 浏览器开发模式下走 mock 不实际写盘。返回最终 save 的绝对路径，取消选择则返回 null。
 */
export async function exportErrorLog(
    suggestedFileName: string,
): Promise<string | null> {
    if (!isTauri) {
        return `~/Downloads/${suggestedFileName}`
    }
    const { save } = await import("@tauri-apps/plugin-dialog")
    const target = await save({
        defaultPath: suggestedFileName,
        filters: [{ name: "Log", extensions: ["log", "txt"] }],
    })
    if (!target) return null
    await invokeOrMock<void>(
        "export_error_log",
        { targetPath: target },
        () => undefined,
    )
    return target
}

export { isTauri }

// ── Coding Agent / Claude 控制台 ───────────────────────────────────────
export type { CodingAgentPermissionMode }

export type McpHealth = "connected" | "failed" | "needs_auth" | "unknown"

export interface McpServerStatus {
    name: string
    detail: string
    health: McpHealth
}

export interface ClaudeDetection {
    installed: boolean
    version: string | null
    exe: string
    mcpServers: McpServerStatus[]
    hasComputerUse: boolean
}

/** 无头 Claude 运行事件，由后端 `coding-agent:test` 流式推送（tag 为 `kind`）。 */
export type CodingAgentEvent =
    | { kind: "started"; session_id: string }
    | { kind: "delta"; session_id: string; text: string }
    | { kind: "tool_use"; session_id: string; name: string }
    | {
          kind: "completed"
          session_id: string
          text: string
          cost_usd: number | null
          duration_ms: number | null
      }
    | { kind: "cancelled"; session_id: string }
    | { kind: "error"; session_id: string; message: string }

export function codingAgentDetect(exe?: string): Promise<ClaudeDetection> {
    return invokeOrMock(
        "coding_agent_detect",
        { exe },
        () => ({
            installed: false,
            version: null,
            exe: exe || "claude",
            mcpServers: [],
            hasComputerUse: false,
        }),
    )
}

export interface CodingAgentRunTestArgs {
    prompt: string
    exe?: string
    permissionMode?: CodingAgentPermissionMode
    workdir?: string
    model?: string
    maxBudgetUsd?: number
}

export function codingAgentRunTest(args: CodingAgentRunTestArgs): Promise<void> {
    return invokeOrMock("coding_agent_run_test", { ...args }, () => undefined)
}

export function codingAgentCancelTest(): Promise<void> {
    return invokeOrMock("coding_agent_cancel_test", undefined, () => undefined)
}

export function codingAgentCommandRisk(command: string): Promise<string | null> {
    return invokeOrMock("coding_agent_command_risk", { command }, () => null)
}

// ── Marketplace (Phase A) ─────────────────────────────────────────────
// 5 个 IPC wrapper —— marketplace-backend HTTP 通过 Rust IPC 转发。Mock fallback
// 让 vite dev 在浏览器里也能预览 UI（返回空列表 / 假数据）。

const MOCK_MARKETPLACE: MarketplaceListItem[] = [
    {
        id: "00000000-0000-0000-0000-000000000001",
        slug: "demo-pack",
        name: "示范风格包",
        description: "Mock 数据 - vite dev 模式下显示",
        authorLogin: "demo",
        version: "1.0.0",
        baseMode: "structured",
        tags: ["demo"],
        likeCount: 12,
        downloadCount: 50,
        publishedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
    },
]

export function listMarketplace(
    options: { query?: string; sort?: "new" | "popular"; limit?: number } = {},
): Promise<MarketplaceListItem[]> {
    return invokeOrMock("marketplace_list", options, () => MOCK_MARKETPLACE)
}

export function fetchMarketplaceDetail(
    packId: string,
): Promise<MarketplaceDetail> {
    return invokeOrMock("marketplace_detail", { packId }, () => ({
        ...MOCK_MARKETPLACE[0],
        prompt: "# 角色\n你是测试用 polish 助手。\n\n# 任务\n按整体意图整理转写。",
        state: "approved" as const,
    }))
}

export function installMarketplacePack(packId: string): Promise<StylePack> {
    return invokeOrMock(
        "marketplace_install",
        { packId },
        () => mockStylePacks[0],
    )
}

export function uploadMarketplacePack(
    packId: string,
    originPackId?: string | null,
): Promise<{ id: string; state: string; message: string }> {
    return invokeOrMock(
        "marketplace_upload",
        { packId, originPackId: originPackId ?? null },
        () => ({
            id: "mock-uploaded",
            state: "pending",
            message: "Mock 上传成功（vite dev）",
        }),
    )
}

export function likeMarketplacePack(
    packId: string,
): Promise<{ likeCount: number; alreadyLiked: boolean }> {
    return invokeOrMock("marketplace_like", { packId }, () => ({
        likeCount: 13,
        alreadyLiked: false,
    }))
}

/** 拉当前登录用户赞过的所有 pack id（用于红心 + 「我赞过的」过滤）。 */
export function marketplaceMyLikes(): Promise<string[]> {
    return invokeOrMock<string[]>("marketplace_my_likes", undefined, () => [])
}

/** 拉当前登录用户发布过的所有 pack（含审核中/已撤回），用于「我的发布」。 */
export function marketplaceMyPacks(): Promise<MarketplaceMyPackItem[]> {
    return invokeOrMock<MarketplaceMyPackItem[]>(
        "marketplace_my_packs",
        undefined,
        () => [],
    )
}

/** 撤回自己发布的 pack（后端软删 state='withdrawn'）。仅允许原作者。 */
export function marketplaceDelete(packId: string): Promise<void> {
    return invokeOrMock<void>("marketplace_delete", { packId }, () => undefined)
}

// ─────────────────────── GitHub OAuth Device Flow (Phase 1) ───────────────
// 客户端直连 GitHub OAuth Device Flow 拿 login，自动写进 prefs.marketplaceDevLogin。
// marketplace backend 不动（继续走 X-Dev-User header；Phase 2 才接 JWT 验证）。
//
// 后端 Rust 实现：commands.rs:github_device_flow_start / github_device_flow_poll
// 需要预先配置 GITHUB_OAUTH_CLIENT_ID（OAuth App client_id，非敏感，可硬编码）。

export interface GithubDeviceStartResponse {
    deviceCode: string
    userCode: string
    verificationUri: string
    interval: number
    expiresIn: number
}

export type GithubDevicePollResult =
    | { kind: "authorized"; login: string }
    | { kind: "pending" }
    | { kind: "slowDown" }
    | { kind: "error"; message: string }

export function githubDeviceFlowStart(): Promise<GithubDeviceStartResponse> {
    return invokeOrMock<GithubDeviceStartResponse>(
        "github_device_flow_start",
        undefined,
        () => ({
            deviceCode: "mock-device-code-xxxxxxxx",
            userCode: "MOCK-CODE",
            verificationUri: "https://github.com/login/device",
            interval: 5,
            expiresIn: 900,
        }),
    )
}

export function githubDeviceFlowPoll(
    deviceCode: string,
): Promise<GithubDevicePollResult> {
    return invokeOrMock<GithubDevicePollResult>(
        "github_device_flow_poll",
        { deviceCode },
        () => ({
            kind: "authorized" as const,
            login: "mock-user",
        }),
    )
}

// ─────────────────────── Marketplace 差量缓存（localStorage） ────────────────
//
// 设计：两段式分发。
// 1) List = 轻量元数据（id + version + updatedAt + 名称 / 计数 / tag），无 prompt 正文。
//    本机持久化，重开 marketplace 秒呈现；后台 refresh 校准。
// 2) Detail = 含 prompt 正文，按 (id, version, updatedAt) 三元组缓存。
//    三元组等价于「内容版本签名」—— version+updatedAt 任一变化 = 内容变了 → 必须重拉。
//    命中 = 复用本机，不发请求；未命中 = fetchMarketplaceDetail 再写回。
// 3) 当 list 里某 pack 消失（被下架 / 撤回）或它的版本签名变了 → 驱逐对应 detail 缓存。
//
// 安全审查（防止恶意服务端 / 缓存投毒 / OOM）：
// - ID 必须是 UUID v4（backend 已强制此约束；客户端镜像校验防 key 注入）。
// - detail.id 必须与请求 packId 一致（防服务端返回错位内容）。
// - 单条 detail 的 prompt 长度上限 200KB（防 OOM via 巨型注入）。
// - detail 缓存条数上限 64，按 LRU 淘汰（防 localStorage 配额耗尽）。
// - List items 在读取 / 写入时按合法 ID 过滤，丢弃格式异常项。

const MARKETPLACE_LIST_CACHE_KEY = "ol-marketplace-list-cache-v2"
const MARKETPLACE_DETAIL_CACHE_KEY = "ol-marketplace-detail-cache-v2"
const MARKETPLACE_LIST_TTL_MS = 24 * 60 * 60 * 1000 // 24h —— list 本来变动稀，refresh 也会自动覆盖
const MARKETPLACE_DETAIL_TTL_MS = 30 * 24 * 60 * 60 * 1000 // 30 天 —— detail 已经按版本三元组锁定，TTL 只是兜底
const MARKETPLACE_DETAIL_MAX_ENTRIES = 64
const MARKETPLACE_DETAIL_MAX_PROMPT_CHARS = 200_000

const PACK_ID_RE =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

function isValidMarketplacePackId(id: unknown): id is string {
    return typeof id === "string" && PACK_ID_RE.test(id)
}

function detailCacheKey(
    id: string,
    version: string,
    updatedAt: string,
): string {
    // version + updatedAt 任一字段为空也能拼出确定 key（refetch 会自然覆盖）。
    return `${id}::${version ?? ""}::${updatedAt ?? ""}`
}

export function readMarketplaceListCache(): MarketplaceListItem[] | null {
    try {
        const raw = localStorage.getItem(MARKETPLACE_LIST_CACHE_KEY)
        if (!raw) return null
        const parsed = JSON.parse(raw) as {
            items: MarketplaceListItem[]
            ts: number
        }
        if (!parsed || !Array.isArray(parsed.items)) return null
        if (Date.now() - parsed.ts > MARKETPLACE_LIST_TTL_MS) return null
        return parsed.items.filter(
            (it) => it && isValidMarketplacePackId(it.id),
        )
    } catch {
        return null
    }
}

export function writeMarketplaceListCache(items: MarketplaceListItem[]): void {
    try {
        const sanitized = items.filter(
            (it) => it && isValidMarketplacePackId(it.id),
        )
        localStorage.setItem(
            MARKETPLACE_LIST_CACHE_KEY,
            JSON.stringify({ items: sanitized, ts: Date.now() }),
        )
        // 服务端最新视图里没有的 (id, version, updatedAt) 一律驱逐 ——
        // 这是「云端哈希被移除时本机也移除」的执行点。
        const keepKeys = new Set(
            sanitized.map((it) =>
                detailCacheKey(it.id, it.version ?? "", it.updatedAt ?? ""),
            ),
        )
        pruneMarketplaceDetailCache(keepKeys)
    } catch {
        // quota exceeded / disabled — silent
    }
}

type MarketplaceDetailCacheEntry = {
    key: string
    detail: MarketplaceDetail
    ts: number
}

function readMarketplaceDetailStore(): Record<
    string,
    MarketplaceDetailCacheEntry
> {
    try {
        const raw = localStorage.getItem(MARKETPLACE_DETAIL_CACHE_KEY)
        if (!raw) return {}
        const parsed = JSON.parse(raw) as Record<
            string,
            MarketplaceDetailCacheEntry
        > | null
        return parsed && typeof parsed === "object" ? parsed : {}
    } catch {
        return {}
    }
}

function writeMarketplaceDetailStore(
    store: Record<string, MarketplaceDetailCacheEntry>,
): void {
    try {
        localStorage.setItem(
            MARKETPLACE_DETAIL_CACHE_KEY,
            JSON.stringify(store),
        )
    } catch {
        // 配额耗尽 — 下次 read 时按 entries 数清理，命中失败会重新走网络。
    }
}

export function readMarketplaceDetailCache(
    packId: string,
    version: string,
    updatedAt: string,
): MarketplaceDetail | null {
    if (!isValidMarketplacePackId(packId)) return null
    const store = readMarketplaceDetailStore()
    const entry = store[detailCacheKey(packId, version, updatedAt)]
    if (!entry) return null
    if (Date.now() - entry.ts > MARKETPLACE_DETAIL_TTL_MS) return null
    if (!entry.detail || entry.detail.id !== packId) return null
    return entry.detail
}

export function writeMarketplaceDetailCache(detail: MarketplaceDetail): void {
    if (!isValidMarketplacePackId(detail.id)) return
    if (
        typeof detail.prompt === "string" &&
        detail.prompt.length > MARKETPLACE_DETAIL_MAX_PROMPT_CHARS
    ) {
        // 巨型 prompt 拒收 —— 防 OOM / 防服务端被攻陷后用大 payload 拖慢客户端。
        return
    }
    const store = readMarketplaceDetailStore()
    const key = detailCacheKey(
        detail.id,
        detail.version ?? "",
        detail.updatedAt ?? "",
    )
    store[key] = { key, detail, ts: Date.now() }
    // LRU: 旧的优先丢
    const entries = Object.values(store).sort((a, b) => a.ts - b.ts)
    while (entries.length > MARKETPLACE_DETAIL_MAX_ENTRIES) {
        const oldest = entries.shift()
        if (oldest) delete store[oldest.key]
    }
    writeMarketplaceDetailStore(store)
}

function pruneMarketplaceDetailCache(keepKeys: Set<string>): void {
    const store = readMarketplaceDetailStore()
    let changed = false
    for (const key of Object.keys(store)) {
        if (!keepKeys.has(key)) {
            delete store[key]
            changed = true
        }
    }
    if (changed) writeMarketplaceDetailStore(store)
}
