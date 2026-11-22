import { useTranslation } from 'react-i18next';
import { type ProviderDescriptor } from '../../api/providers';
import { isExperimentalAsrProvider, isRetiredMacAsrProvider } from '../../lib/experimentalAsr';
import { type OS } from '../../runtime/environment';
import { ASR_LABELS, LLM_LABELS } from './labels';

export type ChannelKind = 'llm' | 'asr';

export interface PresetOption {
  id: string;
  nameKey: string;
  defaultEndpoint?: string;
  endpointPresets?: ProviderDescriptor['endpointPresets'];
  defaultModel?: string;
  authRequirement?: ProviderDescriptor['authRequirement'];
  staticModels?: string[];
  defaultRequestFormat?: ProviderDescriptor['defaultRequestFormat'];
  supportedRequestFormats?: ProviderDescriptor['supportedRequestFormats'];
}

/** Provider list in the "add channel" dropdown. Local engines and Codex OAuth are here too —
 *  they are not fixed preset cards but user-added like cloud vendors, only without key / endpoint fields when editing. */
export function presetsFor(
  kind: ChannelKind,
  os: OS,
  supportsQwen3Mlx = true,
  currentProviderId?: string,
  descriptors: ProviderDescriptor[] = [],
  supportsLocalAsr = true,
): PresetOption[] {
  const descriptorPresets = descriptors.map((descriptor) => ({
    id: descriptor.providerType,
    nameKey: descriptor.labelKey,
    defaultEndpoint: descriptor.defaultEndpoint ?? undefined,
    endpointPresets: descriptor.endpointPresets,
    defaultModel: descriptor.defaultModel ?? undefined,
    authRequirement: descriptor.authRequirement,
    staticModels: descriptor.staticModels,
    defaultRequestFormat: descriptor.defaultRequestFormat,
    supportedRequestFormats: descriptor.supportedRequestFormats,
  }));
  if (kind === 'llm') return descriptorPresets;
  const available = descriptorPresets;
  const visible = available.filter((p) => {
    if (isExperimentalAsrProvider(p.id)) return false;
    if (os === 'mac' && isRetiredMacAsrProvider(p.id)) return false;
    // Keep desktop-only engines off Android.
    if (p.id === 'local-qwen3-mlx') return os === 'mac' && supportsQwen3Mlx;
    if (p.id === 'local-whisper' || p.id === 'apple-speech') return os === 'mac';
    if (p.id === 'local-qwen3-c') return os === 'mac';
    if (p.id === 'local-qwen3') return false;
    if (p.id === 'foundry-local-whisper') return os === 'win';
    // sherpa-onnx also runs on arm64 Android; the capability is ABI-aware.
    if (p.id === 'sherpa-onnx-local') {
      return os === 'win' || (os === 'android' && supportsLocalAsr);
    }
    // The two old Bailian ids are historical aliases; the unified entry is `bailian`, so new cards must not pick them.
    if (p.id === 'bailian-qwen3-realtime' || p.id === 'bailian-fun-asr-flash') return false;
    return true;
  });
  // New channels keep hiding the historical aliases; when editing an existing channel, put the current
  // value back so the Select's value doesn't render empty for a missing option. Only known presets from
  // the registry are accepted; arbitrary strings are not allowed through.
  if (currentProviderId && !visible.some((preset) => preset.id === currentProviderId)) {
    const current = available.find((preset) => preset.id === currentProviderId);
    if (current) visible.push(current);
  }
  return visible;
}

/** Only a fresh draft that never saw user interaction may be recycled as blank. */
export function shouldRecycleDraft(draftId: string | null, touched: boolean): boolean {
  return draftId != null && !touched;
}

/** OrcaRouter channels use the unified brand name without spaces; only fill an empty name, never override user naming. */
export function defaultChannelNameForProvider(providerType: string, currentName: string): string {
  if (currentName.trim() || providerType !== 'orcarouter') return currentName;
  return 'OrcaRouter';
}

export function presetLabel(
  kind: ChannelKind,
  providerType: string,
  t: ReturnType<typeof useTranslation>['t'],
  descriptors: ProviderDescriptor[],
): string {
  const descriptor = descriptors.find((item) => item.providerType === providerType);
  if (descriptor) return t(`settings.providers.presets.${descriptor.labelKey}`);
  const list: readonly { id: string; nameKey: string }[] = kind === 'llm' ? LLM_LABELS : ASR_LABELS;
  const preset = list.find((p) => p.id === providerType);
  return preset ? t(`settings.providers.presets.${preset.nameKey}`) : providerType;
}

/** Credential account read for the model line on the card — kept in sync with ChannelCredentialFields. */
export function modelAccountFor(kind: ChannelKind): string {
  return kind === 'llm' ? 'ark.model_id' : 'asr.model';
}

export function failedOpMessage(error: unknown, fallback: string): string {
  const detail = error instanceof Error ? error.message : String(error);
  const trimmed = detail.trim();
  return trimmed || fallback;
}

/**
 * Compress backend error strings into short labels that fit the button and are actionable:
 * 401 means a wrong key, 429 means rate-limited and retry later, timeout means network —
 * the user must see what to fix.
 */
export function shortErrorLabel(
  raw: string | null,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  const message = (raw ?? '').trim();
  if (message.startsWith('providerHttpStatus:')) {
    return message.split(':')[1] || t('settings.channels.errGeneric');
  }
  // A bare status code is accepted too (history may have stored only "401") — the code itself is the best short label.
  if (/^[1-5]\d{2}$/.test(message)) return message;
  if (message === 'providerRequestTimeout' || message.includes('timeout')) {
    return t('settings.channels.errTimeout');
  }
  if (message === 'providerNetworkError') return t('settings.channels.errNetwork');
  if (message === 'endpointMustUseHttps' || message === 'endpointInvalid') {
    return t('settings.channels.errEndpoint');
  }
  if (message === 'llmModelMissing' || message === 'asrModelMissing') {
    return t('settings.channels.errModel');
  }
  if (message === 'llmThinkingRequired') return t('settings.channels.errThinking');
  return t('settings.channels.errGeneric');
}

/** Layout hit testing must include scrolling between the row and its offset parent,
 * but exclude the row's transient FLIP transform. */
export function channelRowViewportTop(element: HTMLElement): number {
  const parent = element.offsetParent as HTMLElement | null;
  if (!parent) return element.getBoundingClientRect().top;
  let top = parent.getBoundingClientRect().top + parent.clientTop + element.offsetTop;
  for (let ancestor = element.parentElement; ancestor; ancestor = ancestor.parentElement) {
    top -= ancestor.scrollTop;
    if (ancestor === parent) break;
  }
  return top;
}

/** A test result older than a day is stale news; the faded look signals it may no longer hold. */
export const STALE_TEST_SECONDS = 24 * 60 * 60;

/** Keep the row in place until its actual knob animation has settled. */
export function waitForSwitchSlide(control: HTMLElement): Promise<void> {
  return new Promise((resolve) => {
    let frame = 0;
    let settled = false;
    const finish = () => {
      settled = true;
      window.clearTimeout(deadline);
      window.cancelAnimationFrame(frame);
      resolve();
    };
    // A hidden window may stop painting; persistence must still settle.
    const deadline = window.setTimeout(finish, 2000);
    const waitForAnimations = () => {
      if (settled) return;
      const knob = control.querySelector<HTMLElement>('.ol-switch-thumb');
      const animations = (knob?.getAnimations() ?? []).filter(
        (animation) => animation.playState !== 'finished' && animation.playState !== 'idle',
      );
      if (!animations.length) {
        finish();
        return;
      }
      void Promise.allSettled(animations.map((animation) => animation.finished)).then(() => {
        // A quick reversal can replace the transition. Recheck after its events dispatch.
        if (!settled) frame = window.requestAnimationFrame(waitForAnimations);
      });
    };
    frame = window.requestAnimationFrame(() => {
      if (!settled) frame = window.requestAnimationFrame(waitForAnimations);
    });
  });
}

export type ChannelTestMode = 'provider' | 'local-model' | 'unavailable';

export const LOCAL_MODEL_CHANNEL_PROVIDERS = new Set([
  'local-qwen3',
  'local-qwen3-mlx',
  'local-qwen3-c',
  'local-whisper',
  'sherpa-onnx-local',
]);

export function channelTestMode(
  kind: ChannelKind,
  providerType: string,
  validationProbe: string | undefined,
): ChannelTestMode {
  if (kind !== 'asr') return 'provider';
  if (isExperimentalAsrProvider(providerType)) return 'unavailable';
  if (LOCAL_MODEL_CHANNEL_PROVIDERS.has(providerType)) return 'local-model';
  if (
    providerType === 'apple-speech' &&
    (validationProbe == null || validationProbe === 'unsupported')
  ) {
    return 'unavailable';
  }
  return 'provider';
}

export function relativeTime(at: number, t: ReturnType<typeof useTranslation>['t']): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - at);
  if (seconds < 60) return t('settings.channels.justNow');
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t('settings.channels.minutesAgo', { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t('settings.channels.hoursAgo', { count: hours });
  return t('settings.channels.daysAgo', { count: Math.floor(hours / 24) });
}
