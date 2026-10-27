import { common } from '@kit.AbilityKit';
import { pasteboard } from '@kit.BasicServicesKit';
import nativeHost from 'libopenless_harmony.so';
import { HarmonyApi } from '../HarmonyApi';
import {
  ActivityDay,
  Channel,
  CloudSyncStatus,
  CorrectionRule,
  DictionaryEntry,
  HistoryEntry,
  MarketplacePack,
  NativeReply,
  PlatformCapabilities,
  ProviderDescriptor,
  SettingsSnapshot,
  StartupSnapshot,
  StylePack,
  VoiceResult
} from './Types';

/**
 * Typed façade over the shared Rust core.
 *
 * Everything goes through `HarmonyApi.invokeAsync`, which owns the ArkTS-side
 * capabilities (audio capture, microphones, file pickers, credentials, clipboard)
 * and forwards the rest to `libopenless_harmony.so`. The JSON shapes are the
 * camelCase contract from `openless-core::shared_types`, so the native UI and the
 * web UI read exactly the same data.
 */
export class Api {
  private static bridge: HarmonyApi = new HarmonyApi();

  static bridgeOf(): HarmonyApi {
    return Api.bridge;
  }

  static async raw(command: string, argsJson: string): Promise<string> {
    return await Api.bridge.invokeAsync(command, argsJson);
  }

  private static async value(command: string, argsJson: string): Promise<string> {
    const wire = await Api.bridge.invokeAsync(command, argsJson);
    const reply = JSON.parse(wire) as NativeReply;
    if (!reply.ok) {
      const message = reply.error;
      throw new Error(message === undefined || message === null || message.length === 0 ? '后端调用失败' : message);
    }
    if (reply.value === undefined || reply.value === null) return 'null';
    return JSON.stringify(reply.value);
  }

  // ── Startup ────────────────────────────────────────────────────────────────
  static async startupSnapshot(): Promise<StartupSnapshot> {
    return JSON.parse(await Api.value('get_startup_snapshot', '{}')) as StartupSnapshot;
  }

  static async platformCapabilities(): Promise<PlatformCapabilities> {
    return JSON.parse(await Api.value('get_platform_capabilities', '{}')) as PlatformCapabilities;
  }

  static async network(): Promise<NetworkState> {
    const reply = JSON.parse(await Api.raw('check_network', '{}')) as NativeReply;
    if (!reply.ok) return { online: false, latencyMs: null };
    return JSON.parse(JSON.stringify(reply.value)) as NetworkState;
  }

  // ── History ────────────────────────────────────────────────────────────────
  static async history(): Promise<HistoryEntry[]> {
    return JSON.parse(await Api.value('list_history', '{}')) as HistoryEntry[];
  }

  static async deleteHistoryEntry(id: string): Promise<void> {
    await Api.value('delete_history_entry', '{"id":' + JSON.stringify(id) + '}');
  }

  static async clearHistory(): Promise<void> {
    await Api.value('clear_history', '{}');
  }

  static async retranscribe(id: string): Promise<HistoryEntry> {
    return JSON.parse(await Api.value('retranscribe_recording', '{"id":' + JSON.stringify(id) + '}')) as HistoryEntry;
  }

  static async activity(): Promise<ActivityDay[]> {
    return JSON.parse(await Api.value('get_activity_stats', '{}')) as ActivityDay[];
  }

  // ── Settings ───────────────────────────────────────────────────────────────
  static async settings(): Promise<SettingsSnapshot> {
    return JSON.parse(await Api.value('get_settings_snapshot', '{}')) as SettingsSnapshot;
  }

  /**
   * Writes one preference through `update_setting_fields`, which takes JSON
   * pointers (`/llmThinkingEnabled`). ArkTS forbids building a `Record` with a
   * dynamic key, so the pointer and the already-serialized value are spliced into
   * the args document here; every caller passes a literal pointer.
   */
  static async writeSetting(pointer: string, jsonValue: string): Promise<SettingsSnapshot> {
    const args = '{"edits":{"' + pointer + '":' + jsonValue + '}}';
    return JSON.parse(await Api.value('update_setting_fields', args)) as SettingsSnapshot;
  }

  static async writeBool(pointer: string, value: boolean): Promise<SettingsSnapshot> {
    return await Api.writeSetting(pointer, value ? 'true' : 'false');
  }

  static async writeString(pointer: string, value: string): Promise<SettingsSnapshot> {
    return await Api.writeSetting(pointer, JSON.stringify(value));
  }

  static async writeNumber(pointer: string, value: number): Promise<SettingsSnapshot> {
    return await Api.writeSetting(pointer, value.toString());
  }

  // ── Vocabulary ─────────────────────────────────────────────────────────────
  static async vocabulary(): Promise<DictionaryEntry[]> {
    return JSON.parse(await Api.value('list_vocab', '{}')) as DictionaryEntry[];
  }

  static async addVocabulary(phrase: string): Promise<DictionaryEntry> {
    return JSON.parse(await Api.value('add_vocab', '{"phrase":' + JSON.stringify(phrase) + '}')) as DictionaryEntry;
  }

  static async removeVocabulary(id: string): Promise<void> {
    await Api.value('remove_vocab', '{"id":' + JSON.stringify(id) + '}');
  }

  static async setVocabularyEnabled(id: string, enabled: boolean): Promise<void> {
    await Api.value('set_vocab_enabled',
      '{"id":' + JSON.stringify(id) + ',"enabled":' + (enabled ? 'true' : 'false') + '}');
  }

  // ── Correction rules ───────────────────────────────────────────────────────
  static async correctionRules(): Promise<CorrectionRule[]> {
    return JSON.parse(await Api.value('list_correction_rules', '{}')) as CorrectionRule[];
  }

  static async addCorrectionRule(pattern: string, replacement: string): Promise<CorrectionRule> {
    return JSON.parse(await Api.value('add_correction_rule',
      '{"pattern":' + JSON.stringify(pattern) + ',"replacement":' + JSON.stringify(replacement) + '}')) as CorrectionRule;
  }

  static async removeCorrectionRule(id: string): Promise<void> {
    await Api.value('remove_correction_rule', '{"id":' + JSON.stringify(id) + '}');
  }

  static async setCorrectionRuleEnabled(id: string, enabled: boolean): Promise<void> {
    await Api.value('set_correction_rule_enabled',
      '{"id":' + JSON.stringify(id) + ',"enabled":' + (enabled ? 'true' : 'false') + '}');
  }

  // ── Style packs ────────────────────────────────────────────────────────────
  static async stylePacks(): Promise<StylePack[]> {
    return JSON.parse(await Api.value('list_style_packs', '{}')) as StylePack[];
  }

  static async activateStylePack(id: string): Promise<SettingsSnapshot> {
    return JSON.parse(await Api.value('set_active_style_pack', '{"id":' + JSON.stringify(id) + '}')) as SettingsSnapshot;
  }

  static async setStylePackEnabled(id: string, enabled: boolean): Promise<void> {
    await Api.value('set_style_pack_enabled',
      '{"id":' + JSON.stringify(id) + ',"enabled":' + (enabled ? 'true' : 'false') + '}');
  }

  // ── Providers and channels ─────────────────────────────────────────────────
  static async providerDescriptors(kind: string): Promise<ProviderDescriptor[]> {
    return JSON.parse(await Api.value('list_provider_descriptors', '{"kind":' + JSON.stringify(kind) + '}')) as ProviderDescriptor[];
  }

  static async channels(kind: string): Promise<Channel[]> {
    return JSON.parse(await Api.value('list_channels', '{"kind":' + JSON.stringify(kind) + '}')) as Channel[];
  }

  static async activateAsrProvider(provider: string): Promise<SettingsSnapshot> {
    return JSON.parse(await Api.value('set_active_asr_provider', '{"provider":' + JSON.stringify(provider) + '}')) as SettingsSnapshot;
  }

  static async activateLlmProvider(provider: string): Promise<SettingsSnapshot> {
    return JSON.parse(await Api.value('set_active_llm_provider', '{"provider":' + JSON.stringify(provider) + '}')) as SettingsSnapshot;
  }

  static async activateOmniProvider(provider: string): Promise<SettingsSnapshot> {
    return JSON.parse(await Api.value('set_active_omni_provider', '{"provider":' + JSON.stringify(provider) + '}')) as SettingsSnapshot;
  }

  // ── Credentials and channels ───────────────────────────────────────────────
  /**
   * Keys, endpoints and model ids live in the system keystore, keyed by
   * (account, provider-channel). `asrConfigured`/`llmConfigured` in the status
   * tell whether the active channel can actually run.
   */
  static async credentialStatus(): Promise<CredentialStatus> {
    return JSON.parse(await Api.value('get_credentials', '{}')) as CredentialStatus;
  }

  static async setCredential(account: string, value: string, provider: string): Promise<void> {
    await Api.value('set_credential',
      '{"account":' + JSON.stringify(account) + ',"value":' + JSON.stringify(value) +
        ',"provider":' + JSON.stringify(provider) + '}');
  }

  static async createChannel(kind: string, providerType: string, name: string): Promise<Channel> {
    return JSON.parse(await Api.value('create_channel',
      '{"kind":' + JSON.stringify(kind) + ',"providerType":' + JSON.stringify(providerType) +
        ',"name":' + JSON.stringify(name) + '}')) as Channel;
  }

  static async renameChannel(kind: string, id: string, name: string): Promise<void> {
    await Api.value('rename_channel',
      '{"kind":' + JSON.stringify(kind) + ',"id":' + JSON.stringify(id) +
        ',"name":' + JSON.stringify(name) + '}');
  }

  static async deleteChannel(kind: string, id: string): Promise<void> {
    await Api.value('delete_channel',
      '{"kind":' + JSON.stringify(kind) + ',"id":' + JSON.stringify(id) + '}');
  }

  static async setChannelEnabled(kind: string, id: string, enabled: boolean): Promise<void> {
    await Api.value('set_channel_enabled',
      '{"kind":' + JSON.stringify(kind) + ',"id":' + JSON.stringify(id) +
        ',"enabled":' + (enabled ? 'true' : 'false') + '}');
  }

  static async validateChannel(kind: string, channelId: string): Promise<boolean> {
    const wire = await Api.value('validate_provider_credentials',
      '{"kind":' + JSON.stringify(kind) + ',"channelId":' + JSON.stringify(channelId) + '}');
    return wire.indexOf('"ok":true') >= 0;
  }

  static async listModels(kind: string, channelId: string): Promise<string[]> {
    const wire = await Api.value('list_provider_models',
      '{"kind":' + JSON.stringify(kind) + ',"channelId":' + JSON.stringify(channelId) + '}');
    const parsed = JSON.parse(wire) as ModelList;
    return parsed.models ?? [];
  }

  static async recordChannelTest(kind: string, id: string, ok: boolean, latencyMs: number,
    error: string): Promise<void> {
    const payload = '{"kind":' + JSON.stringify(kind) + ',"id":' + JSON.stringify(id) +
      ',"ok":' + (ok ? 'true' : 'false') + ',"latencyMs":' + latencyMs.toString() +
      ',"error":' + (error.length > 0 ? JSON.stringify(error) : 'null') + '}';
    await Api.value('record_channel_test', payload);
  }

  // ── Style packs (write side) ───────────────────────────────────────────────
  static async deleteStylePack(id: string): Promise<void> {
    await Api.value('delete_style_pack', '{"id":' + JSON.stringify(id) + '}');
  }

  static async resetBuiltinStylePack(id: string): Promise<StylePack[]> {
    return JSON.parse(await Api.value('reset_builtin_style_pack', '{"id":' + JSON.stringify(id) + '}')) as StylePack[];
  }

  static async likePack(packId: string): Promise<void> {
    await Api.value('marketplace_like', '{"packId":' + JSON.stringify(packId) + '}');
  }

  /**
   * Style pack archives.
   *
   * Export opens the system save dialog (`DocumentSaveOptions`) with a sensible
   * file name, so the user can drop it into 下载/文档; third-party apps cannot
   * write the public Downloads directory directly - that needs
   * `ohos.permission.READ_WRITE_DOWNLOAD_DIRECTORY`, which only system apps get.
   * Import opens the document picker filtered to `.zip`.
   */
  static async exportStylePack(id: string, name: string): Promise<string> {
    const safeName: string = (name.length > 0 ? name : 'style-pack').replace(/[\\/:*?"<>|]/g, '-');
    const target: string | null = JSON.parse(await Api.value('pick_style_pack_zip_target',
      '{"fileName":' + JSON.stringify('OpenLess-' + safeName + '.zip') + '}')) as string | null;
    const uri: string = target === null ? '' : target;
    if (uri.length === 0) return '已取消';
    const wire = await Api.bridge.invokeAsync('export_style_pack_to_zip',
      '{"id":' + JSON.stringify(id) + ',"targetPath":' + JSON.stringify(uri) + '}');
    const reply = JSON.parse(wire) as NativeReply;
    if (!reply.ok) throw new Error(reply.error ?? '导出失败');
    return '已导出到所选目录';
  }

  static async importStylePack(): Promise<string> {
    const picked: string | null = JSON.parse(await Api.value('pick_style_pack_zip', '{}')) as string | null;
    const uri: string = picked === null ? '' : picked;
    if (uri.length === 0) return '已取消';
    const wire = await Api.bridge.invokeAsync('import_style_pack_from_zip',
      '{"zipPath":' + JSON.stringify(uri) + '}');
    const reply = JSON.parse(wire) as NativeReply;
    if (!reply.ok) throw new Error(reply.error ?? '导入失败');
    return '已导入风格包';
  }

  // ── Re-polish / recordings ─────────────────────────────────────────────────
  static async repolish(rawText: string, stylePackId: string): Promise<string> {
    const payload = '{"rawText":' + JSON.stringify(rawText) +
      ',"stylePackId":' + (stylePackId.length > 0 ? JSON.stringify(stylePackId) : 'null') + '}';
    return JSON.parse(await Api.value('repolish', payload)) as string;
  }

  static async readRecording(sessionId: string): Promise<RecordingPayload> {
    return JSON.parse(await Api.value('read_audio_recording',
      '{"sessionId":' + JSON.stringify(sessionId) + '}')) as RecordingPayload;
  }

  static async saveStylePack(pack: StylePackPayload): Promise<StylePack> {
    return JSON.parse(await Api.value('save_style_pack',
      '{"stylePack":' + JSON.stringify(pack) + '}')) as StylePack;
  }

  // ── Microphone test, diagnostics, GitHub ───────────────────────────────────
  static async startMicrophoneMonitor(): Promise<void> {
    await Api.value('start_microphone_level_monitor', '{}');
  }

  static async stopMicrophoneMonitor(): Promise<void> {
    await Api.value('stop_microphone_level_monitor', '{}');
  }

  static async exportErrorLog(fileName: string): Promise<string> {
    const wire = await Api.value('export_harmony_error_log',
      '{"fileName":' + JSON.stringify(fileName) + '}');
    return JSON.parse(wire) as string;
  }

  static async githubDeviceFlowStart(): Promise<GithubDeviceFlow> {
    return JSON.parse(await Api.value('github_device_flow_start', '{}')) as GithubDeviceFlow;
  }

  static async githubDeviceFlowPoll(flowId: string): Promise<GithubDeviceFlowPoll> {
    return JSON.parse(await Api.value('github_device_flow_poll',
      '{"flowId":' + JSON.stringify(flowId) + '}')) as GithubDeviceFlowPoll;
  }

  /**
   * Learning suggestions live in the backend snapshot (the desktop UI receives them
   * as events; the Harmony host has no event channel, so the snapshot is the source).
   */
  static async pendingCorrections(): Promise<PendingCorrection[]> {
    const wire = await Api.value('get_startup_snapshot', '{}');
    const snapshot = JSON.parse(wire) as SnapshotWithPending;
    const backend = snapshot.backend;
    if (backend === undefined) return [];
    const list = backend.pendingCorrections;
    return list === undefined ? [] : list;
  }

  static async acceptCorrection(id: string): Promise<void> {
    await Api.value('accept_pending_correction', '{"id":' + JSON.stringify(id) + '}');
  }

  static async rejectCorrection(id: string): Promise<void> {
    await Api.value('reject_pending_correction', '{"id":' + JSON.stringify(id) + '}');
  }

  static async dismissSuggestions(): Promise<void> {
    await Api.value('dismiss_vocab_suggestions', '{}');
  }

  /**
   * Vocabulary presets come from the encrypted document store (they are part of the
   * cloud-sync document, with an `origin` per record). The Harmony UI shows them
   * read-only for now: writing them back needs the whole store contract, and a
   * partial round-trip could drop fields the core expects.
   */
  static async vocabularyPresets(): Promise<VocabPreset[]> {
    const wire = await Api.value('list_vocab_presets', '{}');
    const store = JSON.parse(wire) as VocabPresetStore;
    const list = store.presets;
    return list === undefined ? [] : list;
  }

  static async takeSplashPlayback(): Promise<void> {
    try {
      await Api.value('take_splash_playback', '{}');
    } catch (error) {
      console.info('[OpenLess Splash] skipped');
    }
  }

  /** Mirrors the mode chips into the pipeline so dictation picks it up at once. */
  static async setDefaultPolishMode(mode: string): Promise<SettingsSnapshot> {
    return JSON.parse(await Api.value('set_default_polish_mode',
      '{"mode":' + JSON.stringify(mode) + '}')) as SettingsSnapshot;
  }

  static async marketplaceAuthStatus(): Promise<MarketplaceAuth> {
    return JSON.parse(await Api.value('marketplace_auth_status', '{}')) as MarketplaceAuth;
  }

  // ── Cloud sync / marketplace ───────────────────────────────────────────────
  static async cloudSyncStatus(): Promise<CloudSyncStatus> {
    return JSON.parse(await Api.value('cloud_sync_status', '{}')) as CloudSyncStatus;
  }

  /** Uploads the encrypted snapshot built from the current preferences. */
  static async cloudSyncUpload(baseRevision: number, uiPreferencesJson: string): Promise<void> {
    await Api.value('cloud_sync_upload',
      '{"baseRevision":' + baseRevision.toString() + ',"uiPreferences":' + uiPreferencesJson + '}');
  }

  static async cloudSyncRestore(): Promise<void> {
    await Api.value('cloud_sync_restore', '{}');
  }

  static async cloudSyncDelete(baseRevision: number): Promise<void> {
    await Api.value('cloud_sync_delete', '{"baseRevision":' + baseRevision.toString() + '}');
  }

  static async marketplace(query: string, sort: string): Promise<MarketplacePack[]> {
    return JSON.parse(await Api.value('marketplace_list',
      '{"query":' + JSON.stringify(query) + ',"sort":' + JSON.stringify(sort) +
        ',"limit":50}')) as MarketplacePack[];
  }

  static async installMarketplacePack(packId: string): Promise<void> {
    await Api.value('marketplace_install', '{"packId":' + JSON.stringify(packId) + '}');
  }

  // ── Dictation (ArkTS audio capture + shared pipeline) ──────────────────────
  static async startDictation(): Promise<void> {
    await Api.value('start_dictation', '{}');
  }

  static async stopDictation(): Promise<VoiceResult> {
    return JSON.parse(await Api.value('stop_dictation', '{}')) as VoiceResult;
  }

  static async cancelDictation(): Promise<void> {
    await Api.value('cancel_dictation', '{}');
  }

  static async copyText(text: string): Promise<void> {
    const data = pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, text);
    await pasteboard.getSystemPasteboard().setData(data);
  }

  static async microphoneDevices(): Promise<string[]> {
    const wire = JSON.parse(await Api.raw('list_microphone_devices', '{}')) as NativeReply;
    if (!wire.ok) return [];
    const items = JSON.parse(JSON.stringify(wire.value)) as DeviceName[];
    return items.map((item: DeviceName) => item.name);
  }

  static async requestMicrophone(): Promise<string> {
    const wire = JSON.parse(await Api.raw('request_microphone_permission', '{}')) as NativeReply;
    if (!wire.ok) return 'denied';
    return JSON.stringify(wire.value).replace(/"/g, '');
  }

  static async openSystemMicrophoneSettings(): Promise<void> {
    await Api.raw('open_system_settings', '{}');
  }
}

interface DeviceName {
  name: string;
  isDefault: boolean;
}

export interface VocabPreset {
  id: string;
  origin?: string;
  name?: string | null;
  phrases?: string[];
  enabled?: boolean;
  order?: number;
}

interface VocabPresetStore {
  presets?: VocabPreset[];
}

export interface PendingCorrection {
  id: string;
  pattern: string;
  replacement: string;
  source?: string;
  at?: string;
}

interface SnapshotWithPending {
  backend?: PendingBackend;
}

interface PendingBackend {
  pendingCorrections?: PendingCorrection[];
}

export interface MarketplaceAuth {
  loggedIn?: boolean;
  login?: string | null;
}

export interface GithubDeviceFlow {
  flowId: string;
  userCode: string;
  verificationUri: string;
  interval: number;
  expiresIn: number;
}

export interface GithubDeviceFlowPoll {
  state: string;
  login?: string;
  error?: string;
}

export interface ModelList {
  models?: string[];
}

export interface CredentialStatus {
  pipelineMode?: string;
  asrConfigured?: boolean;
  llmConfigured?: boolean;
  omniConfigured?: boolean;
}

export interface RecordingPayload {
  mime?: string;
  fileName?: string;
  base64: string;
  size?: number;
}

export interface StylePackPayload {
  id?: string;
  name: string;
  description: string;
  systemPrompt: string;
  enabled?: boolean;
}

export interface NetworkState {
  online: boolean;
  latencyMs: number | null;
}

/** Convenience wrapper: the app context is needed by the capability layer only. */
export class Runtime {
  static attach(context: common.UIAbilityContext): void {
    Api.bridgeOf().configure(context);
  }
}
