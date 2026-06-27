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
    readCredential,
    validateProviderCredentials,
    listProviderModels,
} from "./asr-credentials"

// history
export {
    listHistory,
    deleteHistoryEntry,
    clearHistory,
    readAudioRecording,
    retranscribeRecording,
} from "./history"

// vocab
export {
    listVocab,
    addVocab,
    removeVocab,
    setVocabEnabled,
    listCorrectionRules,
    addCorrectionRule,
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
    setTranslationHotkey,
    setSwitchStyleHotkey,
    setOpenAppHotkey,
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
    qaWindowPin,
    qaToggleRecording,
    qaSubmitText,
} from "./qa"

// less-computer
export {
    lessComputerWindowDismiss,
    lessComputerApprove,
    lessComputerWindowResize,
} from "./less-computer"

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
} from "./coding-agent"
export type {
    McpServerStatus,
    ClaudeDetection,
    OpenCodeDetection,
    CodingAgentRunTestArgs,
} from "./coding-agent"
export {
    codingAgentDetect,
    codingAgentDetectOpencode,
    codingAgentRunTest,
    codingAgentCancelTest,
    codingAgentCommandRisk,
} from "./coding-agent"

// marketplace
export {
    listMarketplace,
    fetchMarketplaceDetail,
    installMarketplacePack,
    uploadMarketplacePack,
    likeMarketplacePack,
    marketplaceMyLikes,
    marketplaceMyPacks,
    marketplaceDelete,
} from "./marketplace"

// github-oauth
export type { GithubDeviceStartResponse, GithubDevicePollResult } from "./github-oauth"
export { githubDeviceFlowStart, githubDeviceFlowPoll } from "./github-oauth"

// marketplace-cache
export {
    readMarketplaceListCache,
    writeMarketplaceListCache,
    readMarketplaceDetailCache,
    writeMarketplaceDetailCache,
} from "./marketplace-cache"

// utils
export { openExternal, exportErrorLog, logClientError } from "./utils"
