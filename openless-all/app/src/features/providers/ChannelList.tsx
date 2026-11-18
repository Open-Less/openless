// Channel list and editor shared by LLM and ASR.
// Core picks the first enabled channel by sort order; each channel stores credentials
// independently, supporting multiple accounts of the same provider.

import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { testLocalAsrChannel } from '../../api/local-asr';
import {
  createChannel,
  deleteChannelIfBlank,
  listChannels,
  listProviderDescriptors,
  readCredential,
  recordChannelTest,
  reorderChannels,
  setChannelEnabled,
  validateProviderCredentials,
  type Channel,
  type ProviderDescriptor,
} from '../../api/providers';
import { Icon } from '../../components/Icon';
import { Tooltip } from '../../components/Tooltip';
import { PageSkeleton } from '../../components/ui/PageSkeleton';
import { useHotkeySettings } from '../../data/preferences';
import { isExperimentalAsrProvider, isRetiredMacAsrProvider } from '../../lib/experimentalAsr';
import { emitSaved } from '../../lib/savedEvent';
import { toggleGroupFeedback } from '../../lib/toggleFeedback';
import { useExitMount } from '../../lib/useExitMount';
import {
  useConservativeLayout,
  useMobileLayout,
  useReadableLayout,
} from '../../lib/useMobileLayout';
import { useSpringReorder } from '../../lib/useSpringReorder';
import { Button as Btn } from '../../ui/controls/Button';
import { Pill } from '../../ui/controls/Pill';
import { getRuntimeOS } from '../../runtime/environment';
import { getPlatformCapabilities } from '../../runtime/platform';
import { OmniChannelSection } from './ProvidersSection';

import {
  channelTestMode,
  failedOpMessage,
  presetLabel,
  presetsFor,
  relativeTime,
  shortErrorLabel,
  shouldRecycleDraft,
  STALE_TEST_SECONDS,
  waitForSwitchSlide,
  type ChannelKind,
} from './channelCatalog';
import { ChannelEditor } from './ChannelEditor';
import { modelAccountFor } from './channelCatalog';
export {
  channelRowViewportTop,
  channelTestMode,
  defaultChannelNameForProvider,
  presetsFor,
  shouldRecycleDraft,
} from './channelCatalog';

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
  const os = getRuntimeOS();
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

  const isRetired = (channel: Channel) =>
    kind === 'asr' && os === 'mac' && isRetiredMacAsrProvider(channel.providerType);
  // A legacy row can still arrive in imported data. Only eligible channels can be active.
  const activeId = channels.find((channel) => channel.enabled && !isRetired(channel))?.id ?? null;

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
    const notice = `channel-toggle:${kind}:${channel.id}`;
    emitSaved('saving', t('common.saving'), notice);
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
      emitSaved('saved', t('common.saved'), notice);
    } catch (error) {
      console.error('[channels] toggle failed', error);
      await slid;
      await refresh();
      emitSaved('failed', t('common.operationFailed'), notice);
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
          const retired = isRetired(channel);
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
                borderRadius: 'var(--ol-r-lg)',
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
        <ChannelEditor
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
  borderRadius: 'var(--ol-control-radius)',
  background: 'var(--ol-control-solid)',
  color: 'var(--ol-ink-2)',
  cursor: 'pointer',
  fontSize: 12.5,
  fontWeight: 'var(--ol-font-weight-medium)',
};
