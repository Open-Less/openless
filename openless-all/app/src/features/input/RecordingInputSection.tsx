// Recording surfaces under General. One mount owns the device listeners.
// `microphone` is capture plus monitoring. `capsule` is the overlay style.
// `text` is insertion, clipboard, and startup. Those pages stay expanded.

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CapsuleStylePreview } from '../../components/OpenLessCapsule';
import { parseCapsuleStyle } from '../../lib/capsuleLayout';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { playRecordStartCue } from '../../lib/audioCue';
import { defaultDictationHotkey } from '../../lib/hotkey';
import { getSherpaOnnxAsrCatalog } from '../../api/local-asr';
import { isHotkeyModeMigrationNoticeActive } from '../../lib/hotkeyMigration';
import { showWindowsSendInputNewlineMode } from '../../lib/windowsKeyboardListToggle';
import {
  isPlainTextInsertionActive,
  pasteShortcutForPlainTextToggle,
} from '../../lib/plainTextInsertion';
import { isTauriHost } from '../../runtime/environment';
import {
  getAutostartEnabled,
  getVoiceActivationState,
  listMicrophoneDevices,
  listenMicrophoneDeviceChanges,
  listenVoiceActivationChanges,
  setAutostartEnabled,
} from '../../api/input';
import { setDictationHotkey } from '../../api/hotkeys';
import {
  isVoiceActivationModelMissing,
  voiceActivationStatusKey,
  VOICE_ACTIVATION_MODEL_ALIAS,
  type VoiceActivationStateSnapshot,
} from '../../lib/voiceActivation';
import { getPlatformCapabilities, getCachedPlatformCapabilities } from '../../runtime/platform';
import type {
  CapsuleStyle,
  HotkeyMode,
  MicrophoneDevice,
  PasteShortcut,
  PlatformCapabilities,
  UserPreferences,
  WindowsInsertionMode,
  WindowsSendInputNewlineMode,
  MacosNewlineMode,
} from '../../api/contracts';
import { useHotkeySettings } from '../../data/preferences';
import { SelectLite } from '../../components/ui/SelectLite';

import { Card } from '../../ui/layout/Card';
import { Button as Btn } from '../../ui/controls/Button';
import { Segmented } from '../../components/ui/Segmented';
import { SectionTitle } from '../../ui/layout/SectionTitle';
import { SettingRow, Toggle } from '../../ui/controls/SettingRow';
import { inputStyle } from '../../ui/controls/settingsFieldStyles';
import { MicrophoneSelect } from './MicrophoneSelect';
import { getRuntimeOS, type OS } from '../../runtime/environment';

/** Selectable silence durations (seconds) for "auto-stop after silence"; see issue #860. */
const silenceAutoStopOptions = [1, 1.5, 2, 3, 4, 5];

export type RecordingSurface = 'microphone' | 'capsule' | 'text';

export function RecordingInputSection({
  forcedOs,
  onOpenLocalModels,
  surface = 'microphone',
}: {
  forcedOs?: OS;
  onOpenLocalModels?: () => void;
  surface?: RecordingSurface;
} = {}) {
  const { t } = useTranslation();
  const os = forcedOs ?? getRuntimeOS();
  const { prefs, capability, updatePrefs: savePrefs, refresh } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(
    getCachedPlatformCapabilities,
  );
  const [microphoneDevices, setMicrophoneDevices] = useState<MicrophoneDevice[]>([]);
  const [microphoneDevicesLoaded, setMicrophoneDevicesLoaded] = useState(false);
  const [microphoneDevicesError, setMicrophoneDevicesError] = useState<string | null>(null);
  const [voiceActivationModelMissing, setVoiceActivationModelMissing] = useState(false);
  const [voiceActivationState, setVoiceActivationState] =
    useState<VoiceActivationStateSnapshot | null>(null);
  const voiceActivationStateObservedRef = useRef(false);
  const deviceReadRevision = useRef(0);

  useEffect(() => {
    let current = true;
    void getPlatformCapabilities()
      .then((caps) => {
        if (current) setPlatformCaps(caps);
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  }, []);

  const loadMicrophoneDevices = useCallback(
    async (signal?: { cancelled: boolean }, options: { showLoading?: boolean } = {}) => {
      const request = ++deviceReadRevision.current;
      if (options.showLoading ?? true) {
        setMicrophoneDevicesLoaded(false);
      }
      setMicrophoneDevicesError(null);
      try {
        const devices = await listMicrophoneDevices();
        if (signal?.cancelled || deviceReadRevision.current !== request) return;
        setMicrophoneDevices(devices);
        setMicrophoneDevicesLoaded(true);
      } catch (err) {
        console.error('[settings] list microphone devices failed', err);
        if (signal?.cancelled || deviceReadRevision.current !== request) return;
        setMicrophoneDevices([]);
        setMicrophoneDevicesError(err instanceof Error ? err.message : String(err));
        setMicrophoneDevicesLoaded(true);
      }
    },
    [],
  );

  useEffect(() => {
    if (surface !== 'microphone') return;
    const signal = { cancelled: false };
    let unlisten: (() => void) | undefined;
    void (async () => {
      try {
        const stop = await listenMicrophoneDeviceChanges(() => {
          if (!signal.cancelled) void loadMicrophoneDevices(signal, { showLoading: false });
        });
        if (signal.cancelled) stop();
        else unlisten = stop;
      } catch (error) {
        console.warn('[settings] microphone device subscription failed', error);
      }
      if (!signal.cancelled) await loadMicrophoneDevices(signal);
    })();
    return () => {
      signal.cancelled = true;
      unlisten?.();
    };
  }, [loadMicrophoneDevices, surface]);

  useEffect(() => {
    if (!isTauriHost() || os !== 'win') {
      setVoiceActivationModelMissing(false);
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listenVoiceActivationChanges((state) => {
      if (cancelled) return;
      voiceActivationStateObservedRef.current = true;
      setVoiceActivationState((previous) =>
        (previous?.activationGeneration ?? -1) > (state.activationGeneration ?? 0)
          ? previous
          : state,
      );
      setVoiceActivationModelMissing(isVoiceActivationModelMissing(state));
    })
      .then(async (stop) => {
        if (cancelled) {
          stop();
          return;
        }
        unlisten = stop;
        const snapshot = await getVoiceActivationState();
        if (!cancelled && snapshot) {
          setVoiceActivationState((previous) =>
            (previous?.activationGeneration ?? -1) >= (snapshot.activationGeneration ?? 0)
              ? previous
              : snapshot,
          );
        }
      })
      .catch((error) => console.warn('[settings] voice activation subscription failed', error));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [os]);

  useEffect(() => {
    if (os !== 'win' || !prefs?.voiceActivationEnabled) {
      voiceActivationStateObservedRef.current = false;
      setVoiceActivationModelMissing(false);
      return;
    }
    voiceActivationStateObservedRef.current = false;
    let cancelled = false;
    void getSherpaOnnxAsrCatalog()
      .then((catalog) => {
        if (cancelled || voiceActivationStateObservedRef.current) return;
        const model = catalog.find(({ alias }) => alias === VOICE_ACTIVATION_MODEL_ALIAS);
        setVoiceActivationModelMissing(model?.cached === false);
      })
      .catch(() => {
        // The runtime event remains authoritative when the catalog cannot be read.
      });
    return () => {
      cancelled = true;
    };
  }, [os, prefs?.voiceActivationEnabled]);

  useEffect(() => {
    if (!prefs?.voiceActivationEnabled) setVoiceActivationModelMissing(false);
  }, [prefs?.voiceActivationEnabled]);

  useEffect(() => {
    if (isTauriHost() || os !== 'win') return;
    let cancelled = false;
    void getVoiceActivationState()
      .then((snapshot) => {
        if (!cancelled) setVoiceActivationState(snapshot);
      })
      .catch(console.error);
    return () => {
      cancelled = true;
    };
    // The confirmed preference snapshot also changes after an optimistic write.
  }, [os, prefs]);

  const saveWindowsInsertionPrefs = useCallback(
    async (nextPrefs: UserPreferences) => {
      try {
        await savePrefs(nextPrefs);
      } catch (error) {
        // The shared settings provider already reports the save failure and
        // rolls back its optimistic update; do not replace it with an obsolete
        // keyboard-list error when changing the insertion method.
        console.error('[settings] Windows insertion pref save failed', error);
        await refresh();
      }
    },
    [savePrefs, refresh],
  );

  // Read from the cached platform capabilities so the recording controls don't
  // disappear for a frame on return.
  const showDesktopHotkey = platformCaps?.supportsDesktopHotkey === true;

  if (!prefs || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const isAndroid = platformCaps?.platform === 'android';
  const showDesktopInsert = showDesktopHotkey;
  const showDesktopStartup = showDesktopHotkey;
  const plainTextInsertionOn = isPlainTextInsertionActive(os === 'win', prefs.pasteShortcut);
  const effectiveCapsuleDisplayMode =
    os === 'win' ||
    prefs.capsuleDisplayMode === 'cursorScreen' ||
    prefs.capsuleDisplayMode === 'allScreens'
      ? (prefs.capsuleDisplayMode ?? 'cursorScreen')
      : 'cursorScreen';

  const onCapsuleDisplayModeChange = (next: UserPreferences['capsuleDisplayMode']) =>
    savePrefs({ ...prefs, capsuleDisplayMode: next });

  const onModeChange = (mode: HotkeyMode) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, mode } });
  const onShowCapsuleChange = (showCapsule: boolean) => savePrefs({ ...prefs, showCapsule });
  const onMuteDuringRecordingChange = (muteDuringRecording: boolean) =>
    savePrefs({ ...prefs, muteDuringRecording });
  const onAudioCueChange = (audioCueOnRecord: boolean) => savePrefs({ ...prefs, audioCueOnRecord });
  const onLowLatencyDictationChange = (lowLatencyDictationEnabled: boolean) =>
    savePrefs({ ...prefs, lowLatencyDictationEnabled });
  const onMicrophoneDeviceChange = (microphoneDeviceName: string) =>
    savePrefs({ ...prefs, microphoneDeviceName });
  const onRestoreClipboardChange = (restoreClipboardAfterPaste: boolean) =>
    savePrefs({ ...prefs, restoreClipboardAfterPaste });
  const onPasteShortcutChange = (pasteShortcut: PasteShortcut) =>
    savePrefs({ ...prefs, pasteShortcut });
  const onPlainTextInsertionChange = (on: boolean) =>
    savePrefs({ ...prefs, pasteShortcut: pasteShortcutForPlainTextToggle(on) });
  const onAllowNonTsfFallbackChange = (allowNonTsfInsertionFallback: boolean) =>
    savePrefs({ ...prefs, allowNonTsfInsertionFallback });
  const onWindowsInsertionModeChange = (windowsInsertionMode: WindowsInsertionMode) =>
    void saveWindowsInsertionPrefs({
      ...prefs,
      windowsInsertionMode,
      windowsSendInputInsertionOnly: windowsInsertionMode === 'sendInput',
    });
  const onWindowsSendInputNewlineModeChange = (
    windowsSendInputNewlineMode: WindowsSendInputNewlineMode,
  ) => savePrefs({ ...prefs, windowsSendInputNewlineMode });
  const onMacosNewlineModeChange = (macosNewlineMode: MacosNewlineMode) =>
    savePrefs({ ...prefs, macosNewlineMode });
  const onStartMinimizedChange = (startMinimized: boolean) =>
    savePrefs({ ...prefs, startMinimized });
  const onAutoUpdateCheckChange = (autoUpdateCheck: boolean) =>
    savePrefs({ ...prefs, autoUpdateCheck });

  const choices: Array<[HotkeyMode, string]> = [
    ['toggle', t('settings.recording.modeToggle')],
    ['hold', t('settings.recording.modeHold')],
    ['auto', t('settings.recording.modeAuto')],
  ];
  const preferredMicrophoneAvailable = Boolean(
    prefs.microphoneDeviceName &&
    microphoneDevices.some((device) => device.name === prefs.microphoneDeviceName),
  );
  const effectiveMicrophoneDeviceName =
    prefs.microphoneDeviceName && (!microphoneDevicesLoaded || preferredMicrophoneAvailable)
      ? prefs.microphoneDeviceName
      : '';

  return (
    <>
      {surface === 'microphone' && (
        <Card>
          <SectionTitle hint={t('settings.recording.desc')}>
            {t('settings.recording.captureGroupTitle')}
          </SectionTitle>
          {isHotkeyModeMigrationNoticeActive() && showDesktopHotkey && (
            <div
              style={{
                marginTop: 4,
                marginBottom: 8,
                padding: '12px 14px',
                borderRadius: 'var(--ol-r-md)',
                background: 'rgba(37,99,235,0.08)',
                border: '0.5px solid rgba(37,99,235,0.18)',
              }}
            >
              <div
                style={{
                  fontSize: 12.5,
                  fontWeight: 'var(--ol-font-weight-semibold)',
                  color: 'var(--ol-blue)',
                  marginBottom: 4,
                }}
              >
                {t('settings.recording.migrationNoticeTitle')}
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
                {t('settings.recording.migrationNoticeDesc')}
              </div>
            </div>
          )}
          {showDesktopHotkey && (
            <SettingRow
              label={t('settings.recording.hotkeyLabel')}
              desc={os !== 'win' ? t('settings.recording.mouseSideHint') : undefined}
            >
              <ShortcutRecorder
                value={prefs.dictationHotkey}
                sideSpecificModifiers
                allowMouseButtons={os === 'win'}
                onDisable={async () => {
                  await setDictationHotkey(null);
                  await refresh();
                }}
                onSave={async (binding) => {
                  await setDictationHotkey(binding);
                  // setDictationHotkey has already persisted and broadcast the preference
                  // change on the backend; refresh here to pull the latest, avoiding a full
                  // overwrite from the local snapshot that would clobber other preference
                  // changes made in the meantime.
                  await refresh();
                }}
                onReset={async () => {
                  const binding = defaultDictationHotkey();
                  await setDictationHotkey(binding);
                  await refresh();
                }}
              />
            </SettingRow>
          )}
          {showDesktopHotkey && (
            <SettingRow
              label={t('settings.recording.modeLabel')}
              desc={t('settings.recording.modeDesc')}
            >
              <Segmented
                label={t('settings.recording.modeLabel')}
                value={prefs.hotkey.mode}
                onChange={onModeChange}
                options={choices.map(([value, label]) => ({ value, label }))}
              />
            </SettingRow>
          )}
          {showDesktopHotkey && (
            // "Auto-stop after silence" is only available in toggle mode. The outer container
            // always renders: switching the mode from hold/auto to toggle pulls the whole
            // group out from below, and switching away collapses it — the same grid 0fr→1fr
            // animation as the toggle-controlled seconds row, no more "sudden pop-in". While
            // collapsed the content stays in the DOM (overflow hidden), height animates to 0
            // and takes no layout space; inert removes the collapsed controls from tab order
            // and the a11y tree (same as Collapsible; pr_agent feedback).
            <div
              style={{
                display: 'grid',
                gridTemplateRows: prefs.hotkey.mode === 'toggle' ? '1fr' : '0fr',
                transition:
                  'grid-template-rows 0.22s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-quick)',
                opacity: prefs.hotkey.mode === 'toggle' ? 1 : 0,
              }}
              {...(prefs.hotkey.mode !== 'toggle' ? { inert: '' } : {})}
              aria-hidden={prefs.hotkey.mode !== 'toggle'}
            >
              <div style={{ overflow: 'hidden', minHeight: 0 }}>
                <SettingRow
                  label={t('settings.recording.silenceAutoStopLabel')}
                  desc={t('settings.recording.silenceAutoStopDesc')}
                >
                  {/* The toggle row holds only the Toggle: move the seconds dropdown out of
                    this row to avoid jitter from the row widening when toggled. */}
                  <Toggle
                    on={prefs.silenceAutoStopEnabled}
                    onToggle={(next) => savePrefs({ ...prefs, silenceAutoStopEnabled: next })}
                  />
                </SettingRow>
                {/* When enabled, expand the "silence duration" selector row below. Expand/collapse
                uses a grid 0fr→1fr transition (nested in the outer container, pulled
                out/collapsed together with it). The final height is fixed (SettingRow's own
                height), so repeated toggling never jumps. */}
                <div
                  style={{
                    display: 'grid',
                    gridTemplateRows: prefs.silenceAutoStopEnabled ? '1fr' : '0fr',
                    transition:
                      'grid-template-rows 0.22s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-quick)',
                    opacity: prefs.silenceAutoStopEnabled ? 1 : 0,
                  }}
                  {...(!prefs.silenceAutoStopEnabled ? { inert: '' } : {})}
                  aria-hidden={!prefs.silenceAutoStopEnabled}
                >
                  <div style={{ overflow: 'hidden', minHeight: 0 }}>
                    <SettingRow label={t('settings.recording.silenceAutoStopSecondsLabel')}>
                      {/* Same visuals as the microphone dropdown: SelectLite default trigger
                      background + fixed 200px width → right edge aligns with "MacBook Air
                      Microphone" (control areas share the same start and width). */}
                      <SelectLite
                        value={String(
                          silenceAutoStopOptions.includes(prefs.silenceAutoStopSeconds)
                            ? prefs.silenceAutoStopSeconds
                            : 3,
                        )}
                        onChange={(next) =>
                          savePrefs({ ...prefs, silenceAutoStopSeconds: Number(next) })
                        }
                        options={silenceAutoStopOptions.map((value) => ({
                          value: String(value),
                          label: t('settings.recording.silenceAutoStopSecondsValue', { value }),
                        }))}
                        ariaLabel={t('settings.recording.silenceAutoStopSecondsLabel')}
                        style={{ width: 200, maxWidth: '100%', minWidth: 0 }}
                      />
                    </SettingRow>
                  </div>
                </div>
              </div>
            </div>
          )}
          <SettingRow
            label={t('settings.recording.microphoneLabel')}
            desc={t('settings.recording.microphoneDesc')}
          >
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              <MicrophoneSelect
                devices={microphoneDevices}
                selectedName={effectiveMicrophoneDeviceName}
                onSelect={onMicrophoneDeviceChange}
                onOpen={() => {
                  void loadMicrophoneDevices(undefined, { showLoading: false });
                }}
              />
              {microphoneDevicesError && (
                <div style={{ fontSize: 11, color: 'var(--ol-err)', lineHeight: 1.5 }}>
                  {t('settings.recording.microphoneLoadError', { message: microphoneDevicesError })}
                </div>
              )}
            </div>
          </SettingRow>
        </Card>
      )}

      {surface === 'capsule' && !isAndroid && (
        <Card>
          <SectionTitle>{t('settings.recording.capsuleGroupTitle')}</SectionTitle>
          <SettingRow
            label={t('settings.recording.capsuleLabel')}
            desc={t('settings.recording.capsuleDesc')}
          >
            <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
          </SettingRow>
          {(os === 'mac' || os === 'win') && (
            <SettingRow
              label={t('settings.recording.capsuleDisplayModeLabel')}
              desc={t('settings.recording.capsuleDisplayModeDesc')}
            >
              <SelectLite
                value={effectiveCapsuleDisplayMode}
                onChange={(next) =>
                  void onCapsuleDisplayModeChange(next as UserPreferences['capsuleDisplayMode'])
                }
                options={[
                  {
                    value: 'cursorScreen',
                    label: t('settings.recording.capsuleDisplayCursorScreen'),
                  },
                  ...(os === 'win'
                    ? [
                        {
                          value: 'primaryScreen',
                          label: t('settings.recording.capsuleDisplayPrimaryScreen'),
                        },
                        {
                          value: 'foregroundWindowScreen',
                          label: t('settings.recording.capsuleDisplayForegroundWindowScreen'),
                        },
                      ]
                    : []),
                  { value: 'allScreens', label: t('settings.recording.capsuleDisplayAllScreens') },
                ]}
                ariaLabel={t('settings.recording.capsuleDisplayModeLabel')}
                style={{ maxWidth: 240, minWidth: 200 }}
              />
            </SettingRow>
          )}
          <SettingRow label={t('settings.recording.capsuleStyleLabel')}>
            <div style={{ minWidth: 0 }}>
              <Segmented
                options={[
                  { value: 'classic', label: t('settings.recording.capsuleStyleClassic') },
                  { value: 'siri', label: t('settings.recording.capsuleStyleSiri') },
                ]}
                value={parseCapsuleStyle(prefs.capsuleStyle) ?? 'classic'}
                onChange={(next) => savePrefs({ ...prefs, capsuleStyle: next as CapsuleStyle })}
                label={t('settings.recording.capsuleStyleLabel')}
                className="ol-capsule-style-segmented"
              />
              <CapsuleStylePreview style={parseCapsuleStyle(prefs.capsuleStyle) ?? 'classic'} />
            </div>
          </SettingRow>
          <>
            <SettingRow
              label={t('settings.recording.capsuleTranscriptLabel')}
              desc={t('settings.recording.capsuleTranscriptDesc')}
            >
              <Toggle
                on={prefs.capsuleTranscriptEnabled ?? true}
                onToggle={(next) => savePrefs({ ...prefs, capsuleTranscriptEnabled: next })}
              />
            </SettingRow>
            {(prefs.capsuleTranscriptEnabled ?? true) && (
              <SettingRow label={t('settings.recording.capsuleTranscriptFontSize')}>
                <SelectLite
                  value={String(prefs.capsuleTranscriptFontSize ?? 14)}
                  onChange={(next) =>
                    savePrefs({ ...prefs, capsuleTranscriptFontSize: Number(next) })
                  }
                  options={[12, 14, 16, 18, 20].map((size) => ({
                    value: String(size),
                    label: `${size}px`,
                  }))}
                  ariaLabel={t('settings.recording.capsuleTranscriptFontSize')}
                />
              </SettingRow>
            )}
          </>
        </Card>
      )}

      {surface === 'microphone' && (
        <Card>
          <SectionTitle>{t('settings.recording.recognitionGroupTitle')}</SectionTitle>
          <SettingRow
            label={t('settings.recording.stableTranscriptionLabel')}
            desc={t('settings.recording.stableTranscriptionDesc')}
          >
            <Toggle
              on={prefs.stableTranscriptionEnabled}
              onToggle={(next) => savePrefs({ ...prefs, stableTranscriptionEnabled: next })}
            />
          </SettingRow>
          {showDesktopHotkey && os === 'win' && (
            <>
              <SettingRow
                label={t('settings.recording.lowLatencyDictationLabel')}
                desc={t('settings.recording.lowLatencyDictationDesc')}
              >
                <Toggle
                  on={prefs.lowLatencyDictationEnabled ?? false}
                  onToggle={onLowLatencyDictationChange}
                />
              </SettingRow>
              <SettingRow
                label={t('settings.recording.voiceActivationLabel')}
                desc={t('settings.recording.voiceActivationDesc')}
              >
                <Toggle
                  on={prefs.voiceActivationEnabled ?? false}
                  onToggle={(next) => savePrefs({ ...prefs, voiceActivationEnabled: next })}
                />
              </SettingRow>
              {prefs.voiceActivationEnabled && (
                <div
                  style={{
                    margin: '4px 0 10px',
                    padding: '10px 12px',
                    borderRadius: 'var(--ol-r-md)',
                    background: 'var(--ol-surface-2)',
                    border: '0.5px solid var(--ol-line)',
                  }}
                >
                  <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 8 }}>
                    {t('settings.recording.voiceActivationLocalOnly')}
                  </div>
                  <SettingRow
                    label={t('settings.recording.voiceActivationHotkeyLinked')}
                    desc={t('settings.recording.voiceActivationHotkeyLinkedDesc')}
                  >
                    <Toggle
                      on={prefs.voiceActivationHotkeyLinked ?? false}
                      onToggle={(next) =>
                        savePrefs({ ...prefs, voiceActivationHotkeyLinked: next })
                      }
                    />
                  </SettingRow>
                  {prefs.voiceActivationHotkeyLinked && (
                    <div
                      role="status"
                      style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', margin: '6px 0' }}
                    >
                      {voiceActivationState
                        ? t(voiceActivationStatusKey(voiceActivationState))
                        : t('settings.recording.voiceActivationStarting')}
                      {voiceActivationState?.message && ` — ${voiceActivationState.message}`}
                    </div>
                  )}
                  {voiceActivationModelMissing && (
                    <div
                      role="alert"
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 10,
                        marginBottom: 10,
                        padding: '9px 10px',
                        borderRadius: 'var(--ol-control-radius)',
                        background: 'rgba(220, 38, 38, 0.08)',
                        border: '0.5px solid var(--ol-err)',
                      }}
                    >
                      <span style={{ fontSize: 11.5, color: 'var(--ol-err)', lineHeight: 1.5 }}>
                        {t('settings.recording.voiceActivationModelMissing')}
                      </span>
                      {onOpenLocalModels && (
                        <Btn
                          variant="blue"
                          size="sm"
                          onClick={onOpenLocalModels}
                          style={{ flexShrink: 0 }}
                        >
                          {t('settings.recording.voiceActivationDownloadZipformer')}
                        </Btn>
                      )}
                    </div>
                  )}
                  <SettingRow label={t('settings.recording.voiceActivationWakeEnabled')}>
                    <Toggle
                      on={prefs.voiceActivationWakeEnabled ?? true}
                      onToggle={(next) => savePrefs({ ...prefs, voiceActivationWakeEnabled: next })}
                    />
                  </SettingRow>
                  {prefs.voiceActivationWakeEnabled && (
                    <input
                      aria-label={t('settings.recording.voiceActivationWakePhrase')}
                      defaultValue={prefs.voiceActivationWakePhrase ?? '开始语音录入'}
                      onBlur={(event) =>
                        savePrefs({
                          ...prefs,
                          voiceActivationWakePhrase: event.currentTarget.value,
                        })
                      }
                      style={{ ...inputStyle, width: '100%', margin: '2px 0 8px' }}
                    />
                  )}
                  <SettingRow label={t('settings.recording.voiceActivationFinishEnabled')}>
                    <Toggle
                      on={prefs.voiceActivationFinishEnabled ?? true}
                      onToggle={(next) =>
                        savePrefs({ ...prefs, voiceActivationFinishEnabled: next })
                      }
                    />
                  </SettingRow>
                  {prefs.voiceActivationFinishEnabled && (
                    <input
                      aria-label={t('settings.recording.voiceActivationFinishPhrase')}
                      defaultValue={prefs.voiceActivationFinishPhrase ?? '完成语音录入'}
                      onBlur={(event) =>
                        savePrefs({
                          ...prefs,
                          voiceActivationFinishPhrase: event.currentTarget.value,
                        })
                      }
                      style={{ ...inputStyle, width: '100%', margin: '2px 0 8px' }}
                    />
                  )}
                  <SettingRow label={t('settings.recording.voiceActivationTimeoutEnabled')}>
                    <Toggle
                      on={prefs.voiceActivationTimeoutEnabled ?? true}
                      onToggle={(next) =>
                        savePrefs({ ...prefs, voiceActivationTimeoutEnabled: next })
                      }
                    />
                  </SettingRow>
                  {prefs.voiceActivationTimeoutEnabled && (
                    <input
                      type="number"
                      min={5}
                      max={300}
                      aria-label={t('settings.recording.voiceActivationTimeoutSeconds')}
                      defaultValue={prefs.voiceActivationTimeoutSeconds ?? 30}
                      onBlur={(event) =>
                        savePrefs({
                          ...prefs,
                          voiceActivationTimeoutSeconds: Math.max(
                            5,
                            Math.min(300, Number(event.currentTarget.value) || 30),
                          ),
                        })
                      }
                      style={{ ...inputStyle, width: 90, marginTop: 2 }}
                    />
                  )}
                </div>
              )}
            </>
          )}
          <SettingRow
            label={t('settings.recording.muteDuringRecordingLabel')}
            desc={t('settings.recording.muteDuringRecordingDesc')}
          >
            <Toggle on={prefs.muteDuringRecording} onToggle={onMuteDuringRecordingChange} />
          </SettingRow>
          <SettingRow
            label={t('settings.recording.audioCueLabel')}
            desc={t('settings.recording.audioCueDesc')}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <Toggle on={prefs.audioCueOnRecord} onToggle={onAudioCueChange} />
              <button
                type="button"
                onClick={() => playRecordStartCue()}
                style={{
                  padding: '5px 12px',
                  fontSize: 12,
                  fontWeight: 'var(--ol-font-weight-medium)',
                  fontFamily: 'inherit',
                  border: '0.5px solid var(--ol-line-strong)',
                  borderRadius: 'var(--ol-control-radius)',
                  background: 'var(--ol-surface-2)',
                  color: 'var(--ol-ink-2)',
                  cursor: 'default',
                  transition: 'background 0.16s var(--ol-motion-quick)',
                }}
              >
                {t('settings.recording.audioCuePreview')}
              </button>
            </div>
          </SettingRow>
        </Card>
      )}

      {surface === 'text' && showDesktopInsert && (
        <Card>
          <SectionTitle>{t('settings.recording.insertGroupTitle')}</SectionTitle>
          <SettingRow
            label={t('settings.recording.restoreClipboardLabel')}
            desc={t('settings.recording.restoreClipboardDesc')}
          >
            <Toggle on={prefs.restoreClipboardAfterPaste} onToggle={onRestoreClipboardChange} />
          </SettingRow>
          {capability.adapter !== 'macEventTap' && (
            <SettingRow
              label={t('settings.recording.pasteShortcutLabel')}
              desc={t('settings.recording.pasteShortcutDesc')}
            >
              <SelectLite
                value={prefs.pasteShortcut}
                onChange={(next) => onPasteShortcutChange(next as PasteShortcut)}
                // Plain-text insertion owns Shift+Insert, so this control stays read-only.
                disabled={plainTextInsertionOn}
                options={
                  plainTextInsertionOn
                    ? [
                        {
                          value: 'shiftInsert',
                          label: t('settings.recording.pasteShortcutShiftInsert'),
                        },
                      ]
                    : [
                        { value: 'ctrlV', label: t('settings.recording.pasteShortcutCtrlV') },
                        {
                          value: 'ctrlShiftV',
                          label: t('settings.recording.pasteShortcutCtrlShiftV'),
                        },
                      ]
                }
                ariaLabel={t('settings.recording.pasteShortcutLabel')}
                style={{ ...inputStyle, maxWidth: 220 }}
              />
            </SettingRow>
          )}
          {os === 'win' && capability.adapter !== 'macEventTap' && (
            <SettingRow
              label={t('settings.recording.plainTextInsertionLabel')}
              desc={t('settings.recording.plainTextInsertionDesc')}
            >
              <Toggle
                on={plainTextInsertionOn}
                onToggle={onPlainTextInsertionChange}
                label={t('settings.recording.plainTextInsertionLabel')}
              />
            </SettingRow>
          )}
          {capability.adapter === 'windowsLowLevel' && (
            <SettingRow
              label={t('settings.recording.windowsInsertionModeLabel')}
              desc={t('settings.recording.windowsInsertionModeDesc')}
            >
              <SelectLite
                value={
                  prefs.windowsInsertionMode ??
                  (prefs.windowsSendInputInsertionOnly ? 'sendInput' : 'tsf')
                }
                onChange={(next) => onWindowsInsertionModeChange(next as WindowsInsertionMode)}
                options={[
                  { value: 'tsf', label: t('settings.recording.windowsInsertionModeTsf') },
                  {
                    value: 'sendInput',
                    label: t('settings.recording.windowsInsertionModeSendInput'),
                  },
                  { value: 'paste', label: t('settings.recording.windowsInsertionModePaste') },
                ]}
                ariaLabel={t('settings.recording.windowsInsertionModeLabel')}
                style={{ ...inputStyle, maxWidth: 260 }}
              />
            </SettingRow>
          )}
          {capability.adapter === 'windowsLowLevel' &&
            showWindowsSendInputNewlineMode(
              prefs.windowsInsertionMode,
              prefs.windowsSendInputInsertionOnly,
            ) && (
              <SettingRow
                label={t('settings.recording.windowsSendInputNewlineModeLabel')}
                desc={t('settings.recording.windowsSendInputNewlineModeDesc')}
              >
                <SelectLite
                  value={prefs.windowsSendInputNewlineMode ?? 'enter'}
                  onChange={(next) =>
                    onWindowsSendInputNewlineModeChange(next as WindowsSendInputNewlineMode)
                  }
                  options={[
                    {
                      value: 'enter',
                      label: t('settings.recording.windowsSendInputNewlineModeEnter'),
                    },
                    {
                      value: 'shiftEnter',
                      label: t('settings.recording.windowsSendInputNewlineModeShiftEnter'),
                    },
                    {
                      value: 'crlf',
                      label: t('settings.recording.windowsSendInputNewlineModeCrLf'),
                    },
                  ]}
                  ariaLabel={t('settings.recording.windowsSendInputNewlineModeLabel')}
                  style={{ ...inputStyle, maxWidth: 260 }}
                />
              </SettingRow>
            )}
          {capability.adapter === 'macEventTap' && prefs.streamingInsert && (
            <SettingRow
              label={t('settings.recording.macosNewlineModeLabel')}
              desc={t('settings.recording.macosNewlineModeDesc')}
            >
              <SelectLite
                value={prefs.macosNewlineMode ?? 'auto'}
                onChange={(next) => onMacosNewlineModeChange(next as MacosNewlineMode)}
                options={[
                  { value: 'auto', label: t('settings.recording.macosNewlineModeAuto') },
                  {
                    value: 'shiftReturn',
                    label: t('settings.recording.macosNewlineModeShiftReturn'),
                  },
                  { value: 'lineFeed', label: t('settings.recording.macosNewlineModeLineFeed') },
                  { value: 'return', label: t('settings.recording.macosNewlineModeReturn') },
                ]}
                ariaLabel={t('settings.recording.macosNewlineModeLabel')}
                style={{ ...inputStyle, maxWidth: 260 }}
              />
            </SettingRow>
          )}
          {capability.adapter === 'windowsLowLevel' && (
            <SettingRow label={t('settings.recording.allowNonTsfFallbackLabel')}>
              <Toggle
                on={prefs.allowNonTsfInsertionFallback}
                onToggle={onAllowNonTsfFallbackChange}
              />
            </SettingRow>
          )}
          {/* Streaming input: as the polish SSE stream arrives, keystrokes are simulated
            character by character at the cursor to cut perceived latency. Falls back
            automatically to one-shot insertion when conditions aren't met. It's an
            "insertion behavior", hence grouped here. */}
          <SettingRow label={t('settings.advanced.streamingInsertLabel')}>
            <Toggle
              on={!!prefs.streamingInsert}
              onToggle={(next) => void savePrefs({ ...prefs, streamingInsert: next })}
            />
          </SettingRow>
          <SettingRow label={t('settings.advanced.streamingInsertSaveClipboardLabel')}>
            <Toggle
              on={!!prefs.streamingInsertSaveClipboard}
              onToggle={(next) => void savePrefs({ ...prefs, streamingInsertSaveClipboard: next })}
            />
          </SettingRow>
        </Card>
      )}
      {surface === 'text' && showDesktopStartup && (
        <Card>
          <SectionTitle>{t('settings.recording.startupGroupTitle')}</SectionTitle>
          <AutostartRow />
          <SettingRow label={t('settings.recording.startMinimizedLabel')}>
            <Toggle on={prefs.startMinimized} onToggle={onStartMinimizedChange} />
          </SettingRow>
          <SettingRow label={t('settings.recording.autoUpdateCheckLabel')}>
            <Toggle on={prefs.autoUpdateCheck} onToggle={onAutoUpdateCheckChange} />
          </SettingRow>
        </Card>
      )}
    </>
  );
}

// Autostart state is owned by the operating system; it is intentionally not mirrored in prefs.
function AutostartRow() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // Error shown to the user when switching the plist / registry fails. null = no failure /
  // the last operation succeeded.
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauriHost()) {
      setLoaded(true);
      return;
    }
    let cancelled = false;
    getAutostartEnabled()
      .then((v: boolean) => {
        if (!cancelled) {
          setEnabled(v);
          setLoaded(true);
        }
      })
      .catch((err: unknown) => {
        console.error('[autostart] isEnabled failed', err);
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onToggle = async (next: boolean) => {
    setEnabled(next);
    setError(null);
    try {
      await setAutostartEnabled(next);
    } catch (err) {
      console.error('[autostart] toggle failed', err);
      setEnabled(!next);
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <SettingRow
      label={t('settings.recording.startupAtBoot')}
      desc={t('settings.recording.startupAtBootDesc')}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        {loaded ? <Toggle on={enabled} onToggle={onToggle} /> : null}
        {error && (
          <div style={{ fontSize: 11, color: 'var(--ol-err)', marginTop: 4, lineHeight: 1.5 }}>
            {t('settings.recording.startupAtBootError', { message: error })}
          </div>
        )}
      </div>
    </SettingRow>
  );
}
