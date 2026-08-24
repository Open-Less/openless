// Barrel — re-exports every public symbol from the domain modules.
// Must preserve identical exports to the old src/lib/ipc.ts.

export type { UpdateChannel, PlatformCapabilities } from "../types"

// platform & android
export { isAndroid, isDesktop, isMobile } from "./platform-exports"
export {
    getAndroidOverlayStatus,
    requestAndroidOverlayPermission,
    showAndroidOverlay,
    hideAndroidOverlay,
    getAndroidAccessibilityStatus,
    requestAndroidAccessibilityPermission,
} from "./platform-exports"

// shared
export { isTauri, invokeOrMock, getPlatformCapabilities } from "./shared"

// settings
export { getSettings, getDefaultStyleSystemPrompts, setSettings } from "./settings"

// asr-credentials
export type { ProviderCheckResult, ProviderModelsResult } from "./asr-credentials"
export {
    getCredentials,
    setCredential,
    setActiveAsrProvider,
    setActiveLlmProvider,
    setActiveOmniProvider,
    readCredential,
    validateProviderCredentials,
    listProviderModels,
} from "./asr-credentials"

// channels（渠道卡片）
export type { Channel, ChannelKind, ChannelTestResult } from "./channels"
export {
    listChannels,
    createChannel,
    setChannelProviderType,
    deleteChannelIfBlank,
    renameChannel,
    deleteChannel,
    setChannelEnabled,
    reorderChannels,
    recordChannelTest,
} from "./channels"

// history
export {
    listHistory,
    deleteHistoryEntry,
    clearHistory,
    getActivityStats,
    readAudioRecording,
    retranscribeRecording,
} from "./history"

// vocab
export {
    listVocab,
    addVocab,
    addVocabWithMetadata,
    removeVocab,
    setVocabEnabled,
    setVocabMetadata,
    listVocabSuggestions,
    getDictionaryDeliveryPreview,
    listCorrectionRules,
    addCorrectionRule,
    acceptPendingCorrection,
    rejectPendingCorrection,
    dismissVocabSuggestions,
    copyTextToClipboard,
    dismissInsertFallbackCard,
    reportInsertFallbackCardHeight,
    removeCorrectionRule,
    setCorrectionRuleEnabled,
    listVocabPresets,
    saveVocabPresets,
} from "./vocab"

// dictation
export {
    startDictation,
    stopDictation,
    cancelDictation,
    resumeCancelledRecording,
    dismissCancelledRecordingRecovery,
    handleWindowHotkeyEvent,
} from "./dictation"

// style-packs
export {
    repolish,
    setDefaultPolishMode,
    setStyleEnabled,
    listStylePacks,
    saveStylePack,
    createStylePackFromTemplate,
    previewStylePackRuntime,
    setActiveStylePack,
    setStylePackEnabled,
    resetBuiltinStylePack,
    deleteStylePack,
    importStylePackFromZip,
    exportStylePackToZip,
} from "./style-packs"

// permissions
export {
    checkAccessibilityPermission,
    requestAccessibilityPermission,
    checkMicrophonePermission,
    requestMicrophonePermission,
    openSystemSettings,
    triggerMicrophonePrompt,
    restartApp,
    resetAccessibilityPermissionAndRestartApp,
} from "./permissions"

// hotkeys
export {
    getHotkeyStatus,
    getHotkeyCapability,
    getWindowsImeStatus,
    validateComboHotkey,
    setComboHotkey,
    validateShortcutBinding,
    setDictationHotkey,
    setSelectionPolishHotkey,
    setTranslationHotkey,
    setSwitchStyleHotkey,
    setOpenAppHotkey,
    setStylePackHotkeys,
    setShortcutRecordingActive,
} from "./hotkeys"

// devices
export type { NetworkCheckResult } from "./devices"
export {
    checkNetwork,
    listMicrophoneDevices,
    startMicrophoneLevelMonitor,
    stopMicrophoneLevelMonitor,
    isWaylandCliMode,
} from "./devices"

// qa
export {
    getQaHotkeyLabel,
    setQaHotkey,
    qaWindowDismiss,
    qaToggleRecording,
    qaSubmitText,
} from "./qa"

export {
    getSelectionPolishPreview,
    confirmSelectionPolishPreview,
    cancelSelectionPolishPreview,
} from './selection-polish-preview'

export type {
    SelectionCorrectionAction,
    SelectionCorrectionBubblePayload,
    SelectionCorrectionBubbleState,
} from './selection-correction'
export {
    getSelectionCorrection,
    startSelectionCorrection,
    stopSelectionCorrection,
    cancelSelectionCorrection,
    dismissSelectionCorrection,
} from './selection-correction'

// less-computer
export {
    lessComputerWindowDismiss,
    lessComputerWindowOpen,
    lessComputerApprove,
    lessComputerSubmitText,
    lessComputerSync,
} from "./less-computer"

// chat-panel（QA / Less Computer 共用）
export { chatPanelFocusKeyboard } from "./chat-panel"

// updater
export type { LatestBetaRelease, AppUpdateMetadata } from "./updater"
export {
    getUpdateChannel,
    setUpdateChannel,
    fetchLatestBetaRelease,
    appCheckUpdateWithChannel,
    appDownloadAndInstallAndroidUpdate,
} from "./updater"

// remote-server
export type { RemoteInputStatus } from "./remote-server"
export {
    getRemoteInputStatus,
    listLocalIps,
    regenerateRemotePin,
    setRemoteLocale,
} from "./remote-server"

// coding-agent
export type {
    CodingAgentPermissionMode,
    McpHealth,
    CodingAgentEvent,
    OpenCodeDetection,
    McpServerStatus,
    ClaudeDetection,
    CodingAgentRunTestArgs,
} from "./coding-agent"
export {
    codingAgentDetect,
    codingAgentDetectCli,
    codingAgentDetectOpencode,
    codingAgentListOpencodeModels,
    codingAgentRunTest,
    codingAgentCancelTest,
    codingAgentCommandRisk,
} from "./coding-agent"

// marketplace
export {
    listMarketplace,
    fetchMarketplaceDetail,
    installMarketplacePack,
    downloadMarketplacePack,
    uploadMarketplacePack,
    likeMarketplacePack,
    marketplaceMyLikes,
    marketplaceMyPacks,
    marketplaceDelete,
} from "./marketplace"

// github-oauth
export type {
    GithubDeviceStartResponse,
    GithubDevicePollResult,
    MarketplaceAuthStatus,
} from "./github-oauth"
export {
    githubDeviceFlowStart,
    githubDeviceFlowPoll,
    githubDeviceFlowCancel,
    githubPollIntervalMs,
    githubSlowDownIntervalMs,
    githubFlowExpiresAt,
    marketplaceAuthStatus,
    marketplaceLogout,
} from "./github-oauth"

// marketplace-cache
export {
    readMarketplaceListCache,
    writeMarketplaceListCache,
    readMarketplaceDetailCache,
    writeMarketplaceDetailCache,
} from "./marketplace-cache"

// utils
export { openExternal, exportErrorLog, logClientError, debugReadCursorContext } from "./utils"
