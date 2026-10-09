// Barrel — re-exports every public symbol from the domain modules.
// Must preserve identical exports to the old src/lib/ipc.ts.

export type { UpdateChannel, PlatformCapabilities } from '../types';

// platform & android
export { isAndroid, isDesktop, isMobile } from './platform-exports';
export {
  getAndroidOverlayStatus,
  requestAndroidOverlayPermission,
  showAndroidOverlay,
  hideAndroidOverlay,
  getAndroidAccessibilityStatus,
  requestAndroidAccessibilityPermission,
} from './platform-exports';

// shared
export { isTauri, invokeOrMock, getPlatformCapabilities } from './shared';

// settings
export type { StartupSnapshot } from './settings';
export {
  BACKEND_CONTRACT_VERSION,
  getStartupSnapshot,
  getSettings,
  getSettingsSnapshot,
  updateSettingFields,
  getDefaultStyleSystemPrompts,
  setSettings,
} from './settings';

// asr-credentials
export type { ProviderCheckResult, ProviderModelsResult } from './asr-credentials';
export {
  getCredentials,
  setCredential,
  setActiveAsrProvider,
  setActiveLlmProvider,
  setActiveOmniProvider,
  readCredential,
  validateProviderCredentials,
  listProviderModels,
} from './asr-credentials';

export type { AuthRequirement, ProviderDescriptor, ProviderKind } from './providers';
export { listProviderDescriptors } from './providers';

// channels (channel cards)
export type { Channel, ChannelKind, ChannelTestResult } from './channels';
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
} from './channels';

// history
export {
  listHistory,
  deleteHistoryEntry,
  clearHistory,
  getActivityStats,
  readAudioRecording,
  retranscribeRecording,
  applyQuickNoteRepolish,
} from './history';

// vocab
export {
  listVocab,
  addVocab,
  removeVocab,
  setVocabEnabled,
  updateVocab,
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
} from './vocab';

// dictation
export {
  startDictation,
  stopDictation,
  cancelDictation,
  getCapsuleSnapshot,
  setCapsuleTranscriptVisible,
  handleWindowHotkeyEvent,
} from './dictation';

// style-packs
export {
  repolish,
  setDefaultPolishMode,
  setStyleEnabled,
  listStylePacks,
  saveStylePack,
  setStylePackIcon,
  readStylePackIcon,
  createStylePackFromTemplate,
  previewStylePackRuntime,
  setActiveStylePack,
  setStylePackEnabled,
  resetBuiltinStylePack,
  deleteStylePack,
  importStylePackFromZip,
  exportStylePackToZip,
} from './style-packs';

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
} from './permissions';

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
  setQuickNoteHotkey,
  setStylePackHotkeys,
  setShortcutRecordingActive,
} from './hotkeys';

// devices
export type { NetworkCheckResult } from './devices';
export {
  checkNetwork,
  listMicrophoneDevices,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
  isWaylandCliMode,
} from './devices';

// qa
export {
  getQaHotkeyLabel,
  setQaHotkey,
  qaWindowDismiss,
  qaWindowSetExpanded,
  qaGetSnapshot,
  qaToggleRecording,
  qaSubmitText,
  qaSetEditInstructionMode,
} from './qa';

export {
  getSelectionPolishPreview,
  confirmSelectionPolishPreview,
  cancelSelectionPolishPreview,
} from './selection-polish-preview';

export {
  getSelectionVoiceIntentPrompt,
  confirmSelectionVoiceIntentPrompt,
  cancelSelectionVoiceIntentPrompt,
  getSelectionVoicePreview,
  confirmSelectionVoicePreview,
  revertSelectionVoicePreview,
} from './selection-voice-preview';

export {
  startVoiceEditSession,
  openVoiceEditWindow,
  closeVoiceEditWindow,
  finalizeVoiceEditDictation,
  startVoiceEditInstruction,
  finalizeVoiceEditInstruction,
  commitVoiceEditSession,
  cancelVoiceEditSession,
  getVoiceEditState,
} from './voice-edit-session';
export type { VoiceEditSnapshot } from './voice-edit-session';

// less-computer
export {
  lessComputerWindowDismiss,
  lessComputerWindowOpen,
  lessComputerApprove,
  lessComputerSubmitText,
  lessComputerSync,
  lessComputerVoiceStart,
  lessComputerVoiceStop,
  lessComputerVoiceCancel,
  lessComputerTaskCancel,
} from './less-computer';

// chat-panel (shared by QA / Less Computer)
export { chatPanelFocusKeyboard } from './chat-panel';

// updater
export type { LatestBetaRelease, AppUpdateMetadata } from './updater';
export {
  getUpdateChannel,
  setUpdateChannel,
  fetchLatestBetaRelease,
  appCheckUpdateWithChannel,
  appDownloadAndInstallAndroidUpdate,
} from './updater';

// remote-server
export type { RemoteInputStatus } from './remote-server';
export {
  getRemoteInputStatus,
  listLocalIps,
  regenerateRemotePin,
  setRemoteLocale,
} from './remote-server';

// coding-agent
export type {
  CodingAgentPermissionMode,
  McpHealth,
  CodingAgentEvent,
  OpenCodeDetection,
  McpServerStatus,
  ClaudeDetection,
  CodingAgentRunTestArgs,
} from './coding-agent';
export {
  codingAgentDetect,
  codingAgentDetectCli,
  codingAgentDetectOpencode,
  codingAgentListOpencodeModels,
  codingAgentRunTest,
  codingAgentCancelTest,
  codingAgentCommandRisk,
} from './coding-agent';

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
} from './marketplace';

// github-oauth
export type {
  GithubDeviceStartResponse,
  GithubDevicePollResult,
  MarketplaceAuthStatus,
} from './github-oauth';
export {
  githubDeviceFlowStart,
  githubDeviceFlowPoll,
  githubDeviceFlowCancel,
  githubPollIntervalMs,
  githubSlowDownIntervalMs,
  githubFlowExpiresAt,
  marketplaceAuthStatus,
  marketplaceLogout,
} from './github-oauth';

// marketplace-cache
export {
  readMarketplaceListCache,
  writeMarketplaceListCache,
  readMarketplaceDetailCache,
  writeMarketplaceDetailCache,
} from './marketplace-cache';

// splash (2.0 intro-video first-launch marker)
export { takeSplashPlayback, SPLASH_MAJOR } from './splash';

// utils
export { openExternal, exportErrorLog, logClientError, debugReadCursorContext } from './utils';

// Personal cloud backups use the native credential store and Core-owned data.
export { cloudSyncStatus, cloudSyncUpload, cloudSyncRestore, cloudSyncDelete } from './cloud-sync';
export type { CloudSyncStatus, CloudSyncUiPreferences, CloudSyncRestoreResult } from './cloud-sync';
