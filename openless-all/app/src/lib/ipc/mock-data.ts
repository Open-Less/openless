import type {
    ActivityDay,
    CorrectionRule,
    DictationSession,
    DictionaryEntry,
    HotkeyCapability,
    HotkeyStatus,
    PolishMode,
    StylePack,
    StylePackExample,
    StylePackKind,
    StylePackRuntimeDiagnostics,
    StyleSystemPrompts,
    UserPreferences,
    WindowsImeStatus,
    CredentialsStatus,
    MicrophoneDevice,
} from "../types"
import { OL_DATA } from "../mockData"
import {
    defaultAppShortcutModifiers,
    defaultQaShortcut,
} from "../hotkey"

export let mockSettings: UserPreferences = {
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
    windowsInsertionMode: "tsf",
    windowsSendInputNewlineMode: "enter",
    windowsSendInputInsertionOnly: false,
    windowsShowOpenlessInKeyboardList: true,
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
    codingAgentExe: null,
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
    themeMode: "system",
    updateChannel: "stable",
    streamingInsert: true,
    streamingInsertDefaultMigrated: true,
    streamingInsertSaveClipboard: true,
    showOverviewActivityHeatmap: true,
    autoUpdateCheck: true,
    historyMaxEntries: null,
    recordAudioForDebug: false,
    audioRecordingMaxEntries: null,
    marketplaceBaseUrl: "https://apic.openless.top",
    marketplaceDevLogin: "",
    remoteInputEnabled: false,
    remoteInputPort: 8443,
    remoteInputPin: "000000",
    remoteInputDefaultMode: "toggle",
    androidInsertStrategy: "accessibility",
    androidOverlayTrigger: "background",
    androidOverlayActivationMode: "tap",
    androidOverlayLeftSwipeAction: "translation",
    androidOverlayCancelSwipeDirection: "up",
    androidOverlaySizeDp: 72,
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
3) 只输出整理后的正文，不写"以下是优化结果"之类前缀。

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

export const mockDefaultStyleSystemPrompts: StyleSystemPrompts = {
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

export function makeMockStylePack(
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

export let mockStylePacks: StylePack[] = [
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

export function cloneStylePack(stylePack: StylePack): StylePack {
    return {
        ...stylePack,
        tags: [...stylePack.tags],
        examples: stylePack.examples.map((example) => ({ ...example })),
    }
}

export function cloneMockStylePacks(): StylePack[] {
    return mockStylePacks.map(cloneStylePack)
}

export function composeMockStylePackRuntimeDiagnostics(
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

export function syncMockSettingsFromStylePacks() {
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

export const mockHotkeyCapability: HotkeyCapability = {
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

export const mockCredentialsStatus: CredentialsStatus = {
    activeAsrProvider: "foundry-local-whisper",
    activeLlmProvider: "ark",
    asrConfigured: true,
    llmConfigured: true,
    volcengineConfigured: true,
    arkConfigured: true,
}

export const mockHotkeyStatus: HotkeyStatus = {
    adapter: "windowsLowLevel",
    state: "installed",
    message: "Windows 低层键盘 hook 已安装",
    lastError: null,
}

export const mockWindowsImeStatus: WindowsImeStatus = {
    state: "notWindows",
    usingTsfBackend: false,
    message: "Browser dev mock",
    dllPath: null,
}

export const mockMicrophoneDevices: MicrophoneDevice[] = [
    { name: "Built-in Microphone", isDefault: true },
    { name: "USB Microphone", isDefault: false },
]

export const mockHistory: DictationSession[] = OL_DATA.history.map((h, i) => ({
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

export const mockVocab: DictionaryEntry[] = OL_DATA.vocab.map((v, i) => ({
    id: `vocab-${i}`,
    phrase: v.word,
    note: null,
    enabled: true,
    hits: v.count,
    createdAt: new Date().toISOString(),
}))

export const mockCorrectionRules: CorrectionRule[] = [
    {
        id: "rule-quantity-classifier",
        pattern: "{num}粒",
        replacement: "{num}例",
        enabled: true,
        createdAt: new Date().toISOString(),
    },
]

// ── Style pack mutation helpers ───────────────────────────────────────

export function mockSetSettings(prefs: UserPreferences): void {
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
}

export function mockSetDefaultPolishMode(mode: PolishMode): void {
    const packId = `builtin.${mode}`
    mockStylePacks = mockStylePacks.map((pack) => ({
        ...pack,
        enabled: pack.id === packId ? true : pack.enabled,
        active: pack.id === packId,
    }))
    mockSettings = { ...mockSettings, activeStylePackId: packId }
    syncMockSettingsFromStylePacks()
}

export function mockSetStyleEnabled(mode: PolishMode, enabled: boolean): void {
    const packId = `builtin.${mode}`
    mockStylePacks = mockStylePacks.map((pack) =>
        pack.id === packId ? { ...pack, enabled } : { ...pack },
    )
    syncMockSettingsFromStylePacks()
}

export function mockSaveStylePack(stylePack: StylePack): StylePack {
    mockStylePacks = mockStylePacks.map((pack) =>
        pack.id === stylePack.id ? cloneStylePack(stylePack) : pack,
    )
    syncMockSettingsFromStylePacks()
    return cloneStylePack(
        mockStylePacks.find((pack) => pack.id === stylePack.id) ?? stylePack,
    )
}

export function mockCreateStylePackFromTemplate(template: StylePack): StylePack {
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
}

export function mockSetActiveStylePack(id: string): StylePack {
    mockStylePacks = mockStylePacks.map((pack) => ({
        ...pack,
        enabled: pack.id === id ? true : pack.enabled,
        active: pack.id === id,
    }))
    mockSettings = { ...mockSettings, activeStylePackId: id }
    syncMockSettingsFromStylePacks()
    return cloneStylePack(mockStylePacks.find((pack) => pack.id === id)!)
}

export function mockSetStylePackEnabled(id: string, enabled: boolean): StylePack[] {
    mockStylePacks = mockStylePacks.map((pack) =>
        pack.id === id ? { ...pack, enabled } : { ...pack },
    )
    syncMockSettingsFromStylePacks()
    return cloneMockStylePacks()
}

export function mockResetBuiltinStylePack(id: string): StylePack {
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
}

export function mockDeleteStylePack(id: string): void {
    mockStylePacks = mockStylePacks.filter((pack) => pack.id !== id)
    syncMockSettingsFromStylePacks()
}

export function mockImportStylePackFromZip(zipPath: string): StylePack {
    const seed = Date.now()
    const pack = {
        ...makeMockStylePack(
            `imported.mock-${seed}`,
            "imported",
            "light",
            "导入风格包",
            `从 ${zipPath.split(/[/\\]/).pop() || "ZIP"} 导入的风格包`,
            "你是一个负责把口述整理成清晰、利落、适合社区分享文本的编辑，请完整保留事实，不要补充原文没有的信息。",
            ["导入", "ZIP"],
        ),
        author: "Imported ZIP",
    }
    mockStylePacks = [pack, ...mockStylePacks]
    syncMockSettingsFromStylePacks()
    return cloneStylePack(pack)
}

// ── 活动热力图（浏览器 dev 演示数据）────────────────────────────────────
// 过去一年稀疏分布的日计数，铺出有疏密对比的热力图。种子取日期序号的伪随机，
// 刷新之间保持稳定。
export const mockActivityDays: ActivityDay[] = (() => {
    const days: ActivityDay[] = []
    const today = new Date()
    for (let i = 364; i >= 0; i -= 1) {
        const d = new Date(today)
        d.setDate(today.getDate() - i)
        const seed = Math.abs(Math.sin(i * 12.9898) * 43758.5453) % 1
        if (seed < 0.55) continue
        const count = Math.max(1, Math.round(seed * 22) - 8)
        const iso = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
        days.push({ date: iso, count })
    }
    return days
})()
