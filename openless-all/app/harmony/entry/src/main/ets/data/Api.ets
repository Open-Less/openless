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

  // ── Cloud sync / marketplace ───────────────────────────────────────────────
  static async cloudSyncStatus(): Promise<CloudSyncStatus> {
    return JSON.parse(await Api.value('cloud_sync_status', '{}')) as CloudSyncStatus;
  }

  static async marketplace(query: string): Promise<MarketplacePack[]> {
    return JSON.parse(await Api.value('marketplace_list',
      '{"query":' + JSON.stringify(query) + ',"limit":50}')) as MarketplacePack[];
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
