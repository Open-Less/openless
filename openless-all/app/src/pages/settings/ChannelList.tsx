// Channel list and editor shared by LLM and ASR.
// Core picks the first enabled channel by sort order; each channel stores credentials
// independently, supporting multiple accounts of the same provider.

import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
} from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { Modal } from '../../components/ui/Modal';
import { useOverlayMotion } from '../../lib/motion';
import { toggleGroupFeedback } from '../../lib/toggleFeedback';
import { useSpringReorder } from '../../lib/useSpringReorder';
import { PageSkeleton } from '../../components/ui/PageSkeleton';
import { SelectLite } from '../../components/ui/SelectLite';
import { detectOS, type OS } from '../../components/WindowChrome';
import { testLocalAsrChannel } from '../../lib/localAsr';
import { Tooltip } from '../../components/Tooltip';
import { isExperimentalAsrProvider, isRetiredMacAsrProvider } from '../../lib/experimentalAsr';
import {
  createChannel,
  deleteChannel,
  deleteChannelIfBlank,
  listChannels,
  listProviderDescriptors,
  readCredential,
  recordChannelTest,
  renameChannel,
  reorderChannels,
  setChannelEnabled,
  setChannelProviderType,
  setCredential,
  validateProviderCredentials,
  type Channel,
  type ProviderDescriptor,
} from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import { useExitMount } from '../../lib/useExitMount';
import {
  useMobileLayout,
  useReadableLayout,
  useConservativeLayout,
} from '../../lib/useMobileLayout';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { getPlatformCapabilities } from '../../lib/platform';
import { Btn, Pill } from '../_atoms';
import {
  ChannelCredentialFields,
  ChannelFormRow,
  ChannelSectionHeading,
  LLM_LABELS,
  OmniChannelSection,
} from './ProvidersSection';
import { ProviderFormContext, useProviderForm } from './ProviderForm';
import { ASR_LABELS, inputStyle } from './shared';
import { ChannelEditorHostContext } from './ChannelEditorHostContext';

type ChannelKind = 'llm' | 'asr';

interface PresetOption {
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
    // 本地引擎严格按其实际支持的平台暴露；Android 不展示桌面专有实现。
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

function presetLabel(
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
function modelAccountFor(kind: ChannelKind): string {
  return kind === 'llm' ? 'ark.model_id' : 'asr.model';
}

function failedOpMessage(error: unknown, fallback: string): string {
  const detail = error instanceof Error ? error.message : String(error);
  const trimmed = detail.trim();
  return trimmed || fallback;
}

/**
 * Compress backend error strings into short labels that fit the button and are actionable:
 * 401 means a wrong key, 429 means rate-limited and retry later, timeout means network —
 * the user must see what to fix.
 */
function shortErrorLabel(raw: string | null, t: ReturnType<typeof useTranslation>['t']): string {
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
const STALE_TEST_SECONDS = 24 * 60 * 60;

/** Keep the row in place until its actual knob animation has settled. */
function waitForSwitchSlide(control: HTMLElement): Promise<void> {
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

type ChannelTestMode = 'provider' | 'local-model' | 'unavailable';

const LOCAL_MODEL_CHANNEL_PROVIDERS = new Set([
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

function relativeTime(at: number, t: ReturnType<typeof useTranslation>['t']): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - at);
  if (seconds < 60) return t('settings.channels.justNow');
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t('settings.channels.minutesAgo', { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t('settings.channels.hoursAgo', { count: hours });
  return t('settings.channels.daysAgo', { count: Math.floor(hours / 24) });
}

export function ChannelList({
  kind,
  autoCreateWhenEmpty = false,
}: {
  kind: ChannelKind;
  /** For onboarding: when the list is empty, open the add form directly instead of leaving new users staring at an empty list and a plus. */
  autoCreateWhenEmpty?: boolean;
}) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const readable = useReadableLayout();
  const conservative = useConservativeLayout();
  const preferenceStack = readable || conservative;
  const os = detectOS();
  // Initial false: the authoritative getPlatformCapabilities() value is architecture-aware (Apple Silicon /
  // Intel); starting from os === 'mac' would flash the MLX preset once on Intel Macs when the dropdown
  // opens, then remove it asynchronously. On Apple Silicon the MLX option appears one frame late — acceptable.
  const [supportsQwen3Mlx, setSupportsQwen3Mlx] = useState(false);
  const [supportsLocalAsr, setSupportsLocalAsr] = useState(false);
  const [descriptors, setDescriptors] = useState<ProviderDescriptor[]>([]);
  const presets = presetsFor(kind, os, supportsQwen3Mlx, undefined, descriptors, supportsLocalAsr);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [models, setModels] = useState<Record<string, string>>({});
  const [loaded, setLoaded] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  /** A draft card is created up front when creating (credentials must be written per channel id); the dialog edits it directly. */
  const [draftId, setDraftId] = useState<string | null>(null);
  /** Synced ref to avoid a state-scheduling race between blur-save and closing the dialog. */
  const draftTouchedRef = useRef(false);
  const [creatingBusy, setCreatingBusy] = useState(false);
  // Auto-open only once: after the user cancels, the dialog must not keep chasing them.
  const autoOpenedRef = useRef(false);

  useEffect(() => {
    void getPlatformCapabilities().then((caps) => {
      setSupportsQwen3Mlx(caps.supportsLocalQwen3Mlx);
      setSupportsLocalAsr(caps.supportsLocalAsr);
    });
  }, []);

  useEffect(() => {
    void listProviderDescriptors(kind)
      .then(setDescriptors)
      .catch((error) => console.error('[channels] failed to load provider descriptors', error));
  }, [kind]);

  const refresh = useCallback(async () => {
    try {
      const list = await listChannels(kind);
      setChannels(list);
      setLoaded(true);
      // Broadcast to the service tabs: language models / speech recognition are required config, and the
      // red/yellow status dots on the tabs must refresh after every add/remove/edit/enable toggle.
      window.dispatchEvent(new CustomEvent('ol-channels-changed', { detail: { kind } }));
      // Each card shows its current model name — credentials are isolated per channel, so they are read one by one.
      // Channel counts are single digits; the cost of one concurrent read round is negligible.
      const account = modelAccountFor(kind);
      const entries = await Promise.all(
        list.map(async (channel) => {
          try {
            return [channel.id, (await readCredential(account, channel.id)) ?? ''] as const;
          } catch {
            return [channel.id, ''] as const;
          }
        }),
      );
      setModels(Object.fromEntries(entries));
    } catch (error) {
      console.error('[channels] failed to load', error);
      setLoaded(true);
    }
  }, [kind]);

  useEffect(() => {
    void refresh();
    const onExperimentalChange = () => void refresh();
    window.addEventListener('ol-experimental-asr-channels-changed', onExperimentalChange);
    return () =>
      window.removeEventListener('ol-experimental-asr-channels-changed', onExperimentalChange);
  }, [refresh]);

  // ── Add: one step ──
  // Clicking "add channel" opens the edit dialog directly (provider, name, key, test all inside). The draft
  // card is created in the background only because credentials must be persisted per channel id; it is recycled
  // only if the user closed without any interaction. Once any field changed, keep it — an async blur/debounce
  // save must not race the close flow into deleting the card.
  const startCreate = useCallback(async () => {
    if (creatingBusy) return;
    setCreatingBusy(true);
    draftTouchedRef.current = false;
    try {
      const id = await createChannel(kind, presets[0]?.id ?? '', '');
      setDraftId(id);
      await refresh();
    } catch (error) {
      console.error('[channels] create failed', error);
      const message = failedOpMessage(error, t('common.operationFailed'));
      emitSaved('failed', message);
    } finally {
      setCreatingBusy(false);
    }
  }, [creatingBusy, kind, presets, refresh, t]);

  useEffect(() => {
    if (!autoCreateWhenEmpty || !loaded || autoOpenedRef.current) return;
    if (channels.length === 0) {
      autoOpenedRef.current = true;
      void startCreate();
    }
  }, [autoCreateWhenEmpty, loaded, channels.length, startCreate]);

  // The active one = the first enabled channel (the list is already sorted by order).
  const activeId = channels.find((c) => c.enabled)?.id ?? null;

  // ── Card verification ──
  // Runs only when the user clicks: verification is a real call (LLM sends one real polish request, cloud ASR
  // sends a silent audio clip, local ASR loads and transcribes built-in audio). Auto-verifying all cards on
  // settings open would burn quota per card count each time and easily trip rate limits.
  const [testingIds, setTestingIds] = useState<Record<string, boolean>>({});

  const runTest = async (channel: Channel) => {
    if (testingIds[channel.id]) return;
    const mode = channelTestMode(
      kind,
      channel.providerType,
      descriptors.find((item) => item.providerType === channel.providerType)?.validationProbe,
    );
    if (mode === 'unavailable') return;
    setTestingIds((prev) => ({ ...prev, [channel.id]: true }));
    const started = performance.now();
    try {
      const result =
        mode === 'local-model'
          ? await testLocalAsrChannel(channel.id).then(() => ({ ok: true }))
          : await validateProviderCredentials(kind, channel.id);
      const latency = Math.round(performance.now() - started);
      await recordChannelTest(
        kind,
        channel.id,
        result.ok,
        result.ok ? latency : null,
        result.ok ? null : 'validateFailed',
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      try {
        await recordChannelTest(kind, channel.id, false, null, message);
      } catch (recordError) {
        console.error('[channels] failed to record test', recordError);
      }
    } finally {
      setTestingIds((prev) => ({ ...prev, [channel.id]: false }));
      await refresh();
    }
  };

  const onToggle = async (channel: Channel, control: HTMLElement) => {
    const enabled = !channel.enabled;
    emitSaved('saving', t('common.saving'));
    // Flip in place first. Core re-sorts the list (a disabled channel sinks to the end), and
    // moving a row re-inserts its node, which cancels a running transition: the re-sorted
    // list lands only once the knob has finished sliding.
    setChannels((list) =>
      list.map((item) => (item.id === channel.id ? { ...item, enabled } : item)),
    );
    const slid = waitForSwitchSlide(control);
    try {
      await Promise.all([setChannelEnabled(kind, channel.id, enabled), slid]);
      await refresh();
      emitSaved('saved', t('common.saved'));
    } catch (error) {
      console.error('[channels] toggle failed', error);
      await slid;
      await refresh();
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  const [orderNotice, setOrderNotice] = useState('');
  const reorder = useSpringReorder(channels, (next) => {
    setChannels(next);
    setOrderNotice(t('uiRefinement.reordered'));
    void reorderChannels(
      kind,
      next.map((channel) => channel.id),
    )
      .then(refresh)
      .then(() => emitSaved('saved', t('common.saved')))
      .catch((error) => {
        console.error('[channels] reorder failed', error);
        emitSaved('failed', t('common.operationFailed'));
        void refresh();
      });
  });

  const editingChannel = channels.find((c) => c.id === (draftId ?? editingId)) ?? null;
  const isDraft = draftId != null;

  // Dialog exit gating: keep the last opened channel/isDraft during the closing animation so content
  // doesn't vanish mid-animation and the title doesn't flash from "add" back to "edit".
  const dialogMount = useExitMount(editingChannel !== null);
  const lastDialogRef = useRef<{ channel: Channel; isDraft: boolean } | null>(null);
  if (editingChannel) lastDialogRef.current = { channel: editingChannel, isDraft };
  const dialogChannel = editingChannel ?? lastDialogRef.current?.channel ?? null;
  const dialogIsDraft = editingChannel ? isDraft : (lastDialogRef.current?.isDraft ?? false);

  const markDraftTouched = () => {
    if (draftId != null) draftTouchedRef.current = true;
  };

  const closeModal = async () => {
    const id = draftId;
    const touched = draftTouchedRef.current;
    setDraftId(null);
    setEditingId(null);
    draftTouchedRef.current = false;
    if (shouldRecycleDraft(id, touched)) {
      // Recycle only drafts that never saw user interaction; once the user changed anything, the async
      // save — success or failure — must not race the close flow into deleting this card.
      try {
        await deleteChannelIfBlank(kind, id!);
      } catch (error) {
        console.error('[channels] blank cleanup failed', error);
      }
    }
    await refresh();
  };

  return (
    <section
      aria-label={t(`settings.channels.${kind}Title`)}
      style={{ minWidth: 0, marginBottom: 24 }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'flex-start',
          justifyContent: 'space-between',
          flexWrap: 'wrap',
          gap: 16,
          marginBottom: 20,
        }}
      >
        <div style={{ flex: '1 1 260px', minWidth: 0 }}>
          <h2
            style={{
              margin: 0,
              color: 'var(--ol-ink)',
              fontSize: 17,
              fontWeight: 'var(--ol-font-weight-semibold)',
            }}
          >
            {t(`settings.channels.${kind}Title`)}
          </h2>
          <p
            style={{ margin: '7px 0 0', fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.65 }}
          >
            {t('settings.channels.orderHint')}
          </p>
        </div>
        <Btn
          variant="blue"
          icon="plus"
          disabled={creatingBusy || presets.length === 0}
          onClick={() => void startCreate()}
        >
          {creatingBusy ? t('common.loading') : t('settings.channels.add')}
        </Btn>
      </div>

      {!loaded && <PageSkeleton />}
      <span className="ol-sr-only" aria-live="polite">
        {orderNotice}
      </span>
      {loaded && channels.length === 0 && (
        <div style={emptyStyle}>
          <Icon
            name={kind === 'llm' ? 'sparkle' : 'mic'}
            size={24}
            style={{ color: 'var(--ol-blue)', marginBottom: 10 }}
          />
          <div>{t('settings.channels.empty')}</div>
        </div>
      )}

      {/* Active channels no longer use blue background + left bar (the "current" badge already says it;
          a fully tinted row is too loud); rows are rounded cards, selected state is neutral gray fill + thin outline. */}
      <div
        ref={reorder.list}
        className="ol-reorder-list"
        data-ol-toggle-group=""
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 6,
          paddingTop: channels.length ? 12 : 0,
        }}
      >
        {channels.map((channel) => {
          const retired =
            kind === 'asr' && os === 'mac' && isRetiredMacAsrProvider(channel.providerType);
          const isActive = !retired && channel.id === activeId;
          const providerLabel = presetLabel(kind, channel.providerType, t, descriptors);
          const label = channel.name.trim() || providerLabel;
          const model = isExperimentalAsrProvider(channel.providerType)
            ? channel.providerType === 'experimental-textstream'
              ? 'TextStream'
              : channel.providerType === 'experimental-sensevoice'
                ? 'SenseVoice Small'
                : 'Embedding Gemma'
            : (models[channel.id] ?? '');
          const descriptor = descriptors.find((item) => item.providerType === channel.providerType);
          const localEngine = descriptor?.authRequirement === 'none';
          const testMode = retired
            ? 'unavailable'
            : channelTestMode(kind, channel.providerType, descriptor?.validationProbe);
          const content = (
            <div
              key={channel.id}
              data-retired-local-asr={retired || undefined}
              data-channel-id={channel.id}
              data-channel-provider={channel.providerType}
              data-channel-active={isActive}
              ref={(element) => {
                if (element) reorder.rows.current.set(channel.id, element);
                else reorder.rows.current.delete(channel.id);
              }}
              style={{
                display: 'flex',
                flexWrap: 'wrap',
                alignItems: 'center',
                gap: '14px 20px',
                padding: '14px 12px',
                borderRadius: 12,
                border: '0.5px solid',
                borderColor: isActive ? 'var(--ol-line)' : 'transparent',
                background: isActive ? 'var(--ol-surface-2)' : 'transparent',
                position: 'relative',
                opacity: retired ? 0.5 : 1,
                transition:
                  'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 10,
                  minWidth: 0,
                  flex: preferenceStack ? '1 1 100%' : '1 1 260px',
                }}
              >
                <button
                  type="button"
                  className="ol-drag-handle"
                  disabled={retired}
                  onPointerDown={(event) => {
                    if (!retired) reorder.onPointerDown(event, channel.id);
                  }}
                  onKeyDown={(event) => {
                    if (!retired) reorder.onKeyDown(event, channel.id);
                  }}
                  onClick={(event) => event.stopPropagation()}
                  title={t('uiRefinement.reorderHint')}
                  aria-label={`${label} · ${t('settings.channels.dragHint')}`}
                >
                  ⠿
                </button>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 8 }}>
                    <span
                      style={{
                        fontSize: 14,
                        fontWeight: 'var(--ol-font-weight-semibold)',
                        color: 'var(--ol-ink)',
                        overflowWrap: 'anywhere',
                      }}
                    >
                      {label}
                    </span>
                    {isActive && (
                      <Pill tone="blue" size="sm">
                        {t('settings.channels.current')}
                      </Pill>
                    )}
                    {(retired || !channel.enabled) && (
                      <Pill tone="outline" size="sm">
                        {t('settings.channels.disabled')}
                      </Pill>
                    )}
                  </div>
                  <div
                    style={{
                      fontSize: 12,
                      color: 'var(--ol-ink-3)',
                      marginTop: 6,
                      lineHeight: 1.6,
                      overflowWrap: 'anywhere',
                    }}
                  >
                    {channel.name.trim() && <span>{providerLabel} · </span>}
                    <span style={{ fontFamily: model ? 'var(--ol-font-mono)' : undefined }}>
                      {channel.providerType === 'apple-speech'
                        ? t('settings.providers.appleSpeechLocalNote')
                        : model ||
                          t(
                            localEngine
                              ? 'settings.channels.localModelManaged'
                              : 'settings.channels.modelNotSet',
                          )}
                    </span>
                  </div>
                  {retired && <p role="note">{t('experimentalAsr.retiredHint')}</p>}
                  <ChannelTestResult
                    channel={channel}
                    testing={Boolean(testingIds[channel.id])}
                    unavailable={testMode === 'unavailable'}
                    t={t}
                  />
                </div>
              </div>
              <div
                className={conservative ? 'ol-conservative-stack' : undefined}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  flexWrap: 'wrap',
                  gap: 8,
                  marginLeft: preferenceStack ? 0 : 30,
                  width: preferenceStack ? '100%' : undefined,
                }}
              >
                {testMode !== 'unavailable' && (
                  <Btn
                    size="sm"
                    disabled={Boolean(testingIds[channel.id])}
                    onClick={() => void runTest(channel)}
                  >
                    {t(
                      testingIds[channel.id]
                        ? 'settings.channels.verifying'
                        : channel.lastTest && !channel.lastTest.ok
                          ? 'settings.channels.reverify'
                          : 'settings.channels.verify',
                    )}
                  </Btn>
                )}
                <button
                  type="button"
                  role="switch"
                  aria-checked={!retired && channel.enabled}
                  disabled={retired}
                  aria-label={t('settings.channels.enabledFor', { name: label })}
                  onClick={(event) => {
                    if (event.detail > 0) toggleGroupFeedback(event.currentTarget);
                    void onToggle(channel, event.currentTarget);
                  }}
                  style={{ ...ghostBtn, display: 'inline-flex', alignItems: 'center', gap: 7 }}
                >
                  {/* Same track, knob and timing as the settings Toggle, in its small size. */}
                  <span
                    aria-hidden="true"
                    className="ol-switch"
                    data-size="sm"
                    data-checked={channel.enabled && !retired ? '' : undefined}
                  >
                    <span className="ol-switch-thumb" />
                  </span>
                  {t('settings.channels.enabled')}
                </button>
                <button
                  type="button"
                  className="ol-channel-edit-button"
                  onClick={() => setEditingId(channel.id)}
                >
                  <Icon name="pencil" size={14} />
                  {t('settings.channels.edit')}
                  <Icon name="chevRight" size={14} />
                </button>
              </div>
            </div>
          );
          return retired ? (
            <Tooltip
              key={channel.id}
              content={t('experimentalAsr.retiredHint')}
              placement="bottom"
              wrap
              focusable
              trigger={<span />}
            >
              {content}
            </Tooltip>
          ) : (
            content
          );
        })}
      </div>

      {dialogMount.mounted && dialogChannel && (
        <ChannelModal
          key={dialogChannel.id}
          kind={kind}
          channel={dialogChannel}
          presets={presetsFor(
            kind,
            os,
            supportsQwen3Mlx,
            dialogChannel.providerType,
            descriptors,
            supportsLocalAsr,
          )}
          isDraft={dialogIsDraft}
          mobile={mobile}
          closing={dialogMount.closing}
          onClose={() => void closeModal()}
          onChanged={refresh}
          onUserMutation={markDraftTouched}
        />
      )}
    </section>
  );
}

/** Selection and the last manual test are separate facts. The action keeps a
 * stable label; result, elapsed time and age remain readable alongside it. */
function ChannelTestResult({
  channel,
  testing,
  unavailable,
  t,
}: {
  channel: Channel;
  testing: boolean;
  unavailable: boolean;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const last = channel.lastTest;
  const stale = last != null && Math.floor(Date.now() / 1000) - last.at > STALE_TEST_SECONDS;
  const passed = last?.ok;
  const elapsed = last?.latencyMs;
  return (
    <div
      role="status"
      style={{
        display: 'flex',
        alignItems: 'baseline',
        flexWrap: 'wrap',
        gap: '3px 8px',
        marginTop: 7,
        fontSize: 11.5,
        lineHeight: 1.6,
        color: 'var(--ol-ink-3)',
      }}
    >
      <span>
        {t(
          unavailable ? 'settings.channels.verificationUnavailable' : 'settings.channels.lastCheck',
        )}
      </span>
      {unavailable ? null : testing ? (
        <span>{t('settings.channels.verifying')}</span>
      ) : !last ? (
        <span>{t('settings.channels.notVerified')}</span>
      ) : (
        <>
          <span
            style={{
              color: stale ? 'var(--ol-ink-2)' : passed ? 'var(--ol-ok)' : 'var(--ol-err)',
            }}
          >
            {passed
              ? t('settings.channels.passed')
              : shortErrorLabel(last?.error ?? null, t) === t('settings.channels.errGeneric')
                ? t('settings.channels.failedPlain')
                : t('settings.channels.failed', {
                    reason: shortErrorLabel(last?.error ?? null, t),
                  })}
          </span>
          {!passed && (
            <span style={{ color: 'var(--ol-ink-2)' }}>
              {t('settings.channels.failureKeepsEnabled')}
            </span>
          )}
          {passed && elapsed != null && (
            <span>{t('settings.channels.elapsed', { ms: elapsed })}</span>
          )}
          {last && (
            <time
              dateTime={new Date(last.at * 1000).toISOString()}
              title={new Date(last.at * 1000).toLocaleString()}
            >
              {relativeTime(last.at, t)}
            </time>
          )}
          {stale && <span>{t('settings.channels.staleResult')}</span>}
        </>
      )}
    </div>
  );
}

/** Provides the LLM/ASR channel lists for settings and onboarding. */
export function ProvidersSection({
  kind = 'all',
  autoCreateWhenEmpty = false,
}: {
  kind?: 'all' | 'llm' | 'asr';
  autoCreateWhenEmpty?: boolean;
} = {}) {
  const { t } = useTranslation();
  const { prefs } = useHotkeySettings();
  // Onboarding uses the active pipeline; settings can inspect either configuration at any time.
  const multimodalMode =
    prefs?.multimodalPipelineEnabled === true && prefs?.pipelineMode === 'multimodal';
  return (
    <>
      {kind === 'all' && multimodalMode && <OmniChannelSection />}
      {kind === 'all' && !multimodalMode && (
        <div
          style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginBottom: 10 }}
        >
          {t('settings.providers.credentialStorageNotice')}
        </div>
      )}
      {(kind === 'llm' || (kind === 'all' && !multimodalMode)) && (
        <ChannelList kind="llm" autoCreateWhenEmpty={autoCreateWhenEmpty} />
      )}
      {(kind === 'asr' || (kind === 'all' && !multimodalMode)) && (
        <ChannelList kind="asr" autoCreateWhenEmpty={autoCreateWhenEmpty} />
      )}
    </>
  );
}

/**
 * Add and edit share the same form. Mounted as a right-side subpage in settings, or a standalone dialog in onboarding.
 * Creating a channel first obtains the ID the credentials belong to; on close only blank drafts that never saw user action are recycled.
 */
function ChannelModal({
  kind,
  channel,
  presets,
  isDraft,
  mobile,
  closing = false,
  onClose,
  onChanged,
  onUserMutation,
}: {
  kind: ChannelKind;
  channel: Channel;
  presets: PresetOption[];
  /** Draft card in the create flow: title says "add channel"; recyclable while untouched. */
  isDraft: boolean;
  mobile: boolean;
  /** Keeps the editor mounted until its exit finishes. */
  closing?: boolean;
  onClose: () => void;
  onChanged: () => void | Promise<void>;
  /** The user made a meaningful change to the draft; must fire synchronously before async writes. */
  onUserMutation: () => void;
}) {
  const { t } = useTranslation();
  // Drain the channel form's unwritten async writes before closing / switching provider (#1044 semantics).
  const form = useProviderForm();
  const [name, setName] = useState(channel.name);
  const [providerType, setProviderType] = useState(channel.providerType);
  const [changingProvider, setChangingProvider] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const nameId = useId();
  const editorHost = useContext(ChannelEditorHostContext);
  const embedded = Boolean(editorHost?.container && editorHost.background);
  useOverlayMotion(dialogRef, closing, 'drawer', embedded);
  const closeRequestedRef = useRef(false);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const requestClose = () => {
    if (closeRequestedRef.current) return;
    void form.finish(() => {
      if (closeRequestedRef.current) return;
      closeRequestedRef.current = true;
      onCloseRef.current();
    });
  };

  useEffect(() => {
    const opener = document.activeElement;
    const dialog = dialogRef.current;
    const parentDialog =
      opener instanceof HTMLElement ? opener.closest<HTMLElement>('[role="dialog"]') : null;
    const background = embedded
      ? editorHost?.background
      : parentDialog !== dialog
        ? parentDialog
        : null;
    const wasInert = background?.inert ?? false;
    (
      dialog?.querySelector<HTMLElement>('[role="combobox"], input:not([disabled])') ?? dialog
    )?.focus({ preventScroll: true });
    // Keep body-portaled provider menus accessible while disabling the covered
    // settings surface. aria-modal would hide those existing sibling portals.
    if (background) background.inert = true;
    if (embedded) editorHost?.registerClose(requestClose);
    return () => {
      if (embedded) editorHost?.registerClose(null);
      if (background) background.inert = wasInert;
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, []);

  const onDialogKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const dialog = dialogRef.current;
    if (!dialog || event.defaultPrevented) return;
    if (event.target instanceof Element && event.target.closest('[role="dialog"]') !== dialog)
      return;
    if (event.key === 'Escape') {
      // SelectLite handles its open menu first. Never close both layers at once.
      if (dialog.querySelector('[role="combobox"][aria-expanded="true"]')) return;
      event.preventDefault();
      event.stopPropagation();
      requestClose();
    } else if (event.key === 'Tab') {
      const controls = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => element.tabIndex >= 0 && element.getClientRects().length > 0);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!first) {
        event.preventDefault();
        dialog.focus();
      } else if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === dialog)
      ) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  };

  const saveName = async () => {
    if (name.trim() === channel.name.trim()) return;
    try {
      await renameChannel(kind, channel.id, name.trim());
      await onChanged();
    } catch (error) {
      console.error('[channels] rename failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  // After switching providers, write the preset's default endpoint / model into EMPTY slots (never overwrite
  // user-entered custom values). Channels are isolated per card, so this reads/writes by card id; Codex OAuth /
  // local engines / custom OpenAI-compatible (empty baseUrl/model) naturally skip. Failures are logged only.
  const fillProviderDefaults = async (next: string) => {
    try {
      const preset = presets.find((item) => item.id === next);
      if (!preset) return;
      const endpointAccount = kind === 'llm' ? 'ark.endpoint' : 'asr.endpoint';
      const modelAccount = kind === 'llm' ? 'ark.model_id' : 'asr.model';
      if (next === 'orcarouter') {
        if (preset.defaultEndpoint)
          await setCredential(endpointAccount, preset.defaultEndpoint, channel.id);
        if (preset.defaultModel) await setCredential(modelAccount, preset.defaultModel, channel.id);
        return;
      }
      if (preset.defaultEndpoint && !(await readCredential(endpointAccount, channel.id))?.trim()) {
        await setCredential(endpointAccount, preset.defaultEndpoint, channel.id);
      }
      if (preset.defaultModel && !(await readCredential(modelAccount, channel.id))?.trim()) {
        await setCredential(modelAccount, preset.defaultModel, channel.id);
      }
    } catch (error) {
      console.error('[channels] failed to fill provider defaults', error);
    }
  };

  const changeProvider = async (next: string) => {
    const previous = providerType;
    onUserMutation();
    await form.finish(() => undefined);
    setChangingProvider(true);
    try {
      await setChannelProviderType(kind, channel.id, next);
      await fillProviderDefaults(next);
      setProviderType(next);
      const defaultName = defaultChannelNameForProvider(next, name);
      if (defaultName !== name) {
        await renameChannel(kind, channel.id, defaultName);
        setName(defaultName);
      }
      await onChanged();
    } catch (error) {
      console.error('[channels] change provider failed', error);
      setProviderType(previous);
      emitSaved('failed', t('common.operationFailed'));
    } finally {
      setChangingProvider(false);
    }
  };

  const remove = async () => {
    try {
      await deleteChannel(kind, channel.id);
      emitSaved('saved', t('common.saved'));
      requestClose();
    } catch (error) {
      console.error('[channels] delete failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  const descriptor = presets.find((item) => item.id === providerType);
  const isLocalEngine = descriptor?.authRequirement === 'none' && providerType !== 'apple-speech';

  const content = (
    <ProviderFormContext.Provider value={form}>
      <div
        ref={dialogRef}
        className={`ol-channel-dialog${embedded ? ' ol-channel-dialog-embedded' : ''}`}
        data-closing={closing ? 'true' : undefined}
        role="dialog"
        aria-label={t(isDraft ? 'settings.channels.createTitle' : 'settings.channels.editTitle')}
        tabIndex={-1}
        onKeyDown={onDialogKeyDown}
      >
        <header className="ol-channel-dialog-header">
          {embedded ? (
            <button
              type="button"
              className="ol-settings-back"
              onClick={requestClose}
              aria-label={t('settings.channels.backToList')}
              title={t('settings.channels.backToList')}
            >
              <Icon name="chevLeft" size={19} />
            </button>
          ) : (
            <span className="ol-channel-dialog-icon">
              <Icon name={kind === 'llm' ? 'style' : 'mic'} size={22} />
            </span>
          )}
          <div className="ol-channel-dialog-heading">
            <div className="ol-channel-dialog-title">
              <h2>
                {t(isDraft ? 'settings.channels.createTitle' : 'settings.channels.editTitle')}
              </h2>
              <span>{t(`settings.channels.${kind}Title`)}</span>
            </div>
            <p>{t('settings.channels.autoSaveHint')}</p>
          </div>
          {!embedded && (
            <button
              type="button"
              className="ol-channel-dialog-close"
              onClick={requestClose}
              aria-label={t('common.close')}
            >
              <Icon name="close" size={18} />
            </button>
          )}
        </header>

        <div className="ol-channel-dialog-body ol-thinscroll">
          <div className="ol-channel-form-sheet">
            <ChannelSectionHeading icon="cloud" title={t('settings.channels.connectionTitle')} />
            <ChannelFormRow label={t('settings.channels.providerLabel')}>
              <SelectLite
                value={providerType}
                disabled={changingProvider || form.leaving}
                onChange={(next) => void changeProvider(next)}
                options={presets.map((p) => ({
                  value: p.id,
                  label: t(`settings.providers.presets.${p.nameKey}`),
                }))}
                ariaLabel={t('settings.channels.providerLabel')}
                style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
              />
            </ChannelFormRow>
            <ChannelFormRow label={t('settings.channels.nameLabel')} htmlFor={nameId}>
              <input
                id={nameId}
                value={name}
                onChange={(e) => {
                  onUserMutation();
                  setName(e.target.value);
                }}
                onBlur={() => void saveName()}
                placeholder={t('settings.channels.namePlaceholder')}
                style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
              />
              <p className="ol-channel-name-hint">{t('settings.channels.nameHint')}</p>
            </ChannelFormRow>

            {/* Model list, provider-specific fields, and test results all stay in the same scroll area. */}
            {!changingProvider && (
              <ChannelCredentialFields
                key={`${channel.id}:${providerType}`}
                kind={kind}
                providerType={providerType}
                channelId={channel.id}
                descriptor={descriptor}
                onTested={() => void onChanged()}
                onUserMutation={onUserMutation}
              />
            )}
            {isLocalEngine && (
              <p className="ol-channel-local-hint">{t('settings.channels.localEngineModelHint')}</p>
            )}
          </div>
        </div>

        <footer className="ol-channel-dialog-footer">
          {confirmDelete ? (
            <div
              className="ol-channel-delete-confirm"
              role="group"
              aria-label={t('settings.channels.delete')}
            >
              <p>{t('settings.channels.deleteConfirm')}</p>
              <button
                type="button"
                autoFocus
                className="ol-channel-delete-button"
                onClick={() => void remove()}
              >
                <Icon name="trash" size={15} />
                {t('settings.channels.confirmDelete')}
              </button>
              <button type="button" onClick={() => setConfirmDelete(false)} style={ghostBtn}>
                {t('common.cancel')}
              </button>
            </div>
          ) : (
            <button
              type="button"
              className="ol-channel-delete-button"
              onClick={() => setConfirmDelete(true)}
            >
              <Icon name="trash" size={15} />
              {t('settings.channels.delete')}
            </button>
          )}
          <div className="ol-channel-dialog-footer-end">
            <span className="ol-channel-autosave-hint">{t('modal.autoSaveHint')}</span>
            <Btn
              variant={embedded ? 'primary' : 'blue'}
              onClick={requestClose}
              disabled={form.leaving}
            >
              {t(embedded ? 'settings.channels.done' : 'common.close')}
            </Btn>
          </div>
        </footer>
      </div>
    </ProviderFormContext.Provider>
  );
  return embedded ? (
    createPortal(content, editorHost!.container!)
  ) : (
    <Modal
      onClose={requestClose}
      zIndex={1000}
      closing={closing}
      width={mobile ? '100%' : 'min(840px, 100%)'}
      style={{ padding: 0, overflow: 'hidden', maxHeight: 'calc(100dvh - 40px)' }}
    >
      {content}
    </Modal>
  );
}

const emptyStyle: CSSProperties = {
  padding: '28px 20px',
  textAlign: 'center',
  fontSize: 13,
  color: 'var(--ol-ink-3)',
  lineHeight: 1.7,
  borderTop: '1px solid var(--ol-line)',
  borderBottom: '1px solid var(--ol-line)',
};

const ghostBtn: CSSProperties = {
  height: 32,
  padding: '0 14px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8,
  background: 'var(--ol-control-solid)',
  color: 'var(--ol-ink-2)',
  cursor: 'pointer',
  fontSize: 12.5,
  fontWeight: 'var(--ol-font-weight-medium)',
};
