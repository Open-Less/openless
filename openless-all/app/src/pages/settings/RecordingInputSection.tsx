// Recording & input settings: recording mode, audio cues, input behavior, and
// platform-specific options.

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CapsuleStylePreview } from '../../components/OpenLessCapsule';
import { parseCapsuleStyle } from '../../lib/capsuleLayout';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { playRecordStartCue } from '../../lib/audioCue';
import { defaultDictationHotkey } from '../../lib/hotkey';
import { getSherpaOnnxAsrCatalog } from '../../lib/localAsr';
import { emitSaved } from '../../lib/savedEvent';
import { isHotkeyModeMigrationNoticeActive } from '../../lib/hotkeyMigration';
import {
  showWindowsOpenlessKeyboardListToggle,
  showWindowsSendInputNewlineMode,
} from '../../lib/windowsKeyboardListToggle';
import {
  isPlainTextInsertionActive,
  pasteShortcutForPlainTextToggle,
} from '../../lib/plainTextInsertion';
import { isTauri, listMicrophoneDevices, setDictationHotkey } from '../../lib/ipc';
import {
  isVoiceActivationModelMissing,
  VOICE_ACTIVATION_MODEL_ALIAS,
  type VoiceActivationStateSnapshot,
} from '../../lib/voiceActivation';
import { getPlatformCapabilities, getCachedPlatformCapabilities } from '../../lib/platform';
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
} from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite } from '../../components/ui/SelectLite';

import { Card, Collapsible, Btn } from '../_atoms';
import { Segmented } from '../../components/ui/Segmented';
import { SectionTitle, SettingRow, Toggle, inputStyle } from './shared';
import { MicrophoneSelect } from './MicrophoneSelect';
import { AudioDspSettings } from './AudioDspSettings';
import { detectOS, type OS } from '../../components/WindowChrome';

/** Selectable silence durations (seconds) for "auto-stop after silence"; see issue #860. */
const silenceAutoStopOptions = [1, 1.5, 2, 3, 4, 5];

async function autostartIsEnabled(): Promise<boolean> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<boolean>('plugin:autostart|is_enabled');
}

async function autostartEnable(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('plugin:autostart|enable');
}

async function autostartDisable(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('plugin:autostart|disable');
}

export function RecordingInputSection({
  forcedOs,
  onOpenLocalModels,
}: {
  forcedOs?: OS;
  onOpenLocalModels?: () => void;
} = {}) {
  const { t } = useTranslation();
  const os = forcedOs ?? detectOS();
  const { prefs, capability, updatePrefs: savePrefs, refresh } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(
    getCachedPlatformCapabilities,
  );
  const [microphoneDevices, setMicrophoneDevices] = useState<MicrophoneDevice[]>([]);
  const [microphoneDevicesLoaded, setMicrophoneDevicesLoaded] = useState(false);
  const [microphoneDevicesError, setMicrophoneDevicesError] = useState<string | null>(null);
  const [voiceActivationModelMissing, setVoiceActivationModelMissing] = useState(false);
  const voiceActivationStateObservedRef = useRef(false);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const loadMicrophoneDevices = useCallback(
    async (signal?: { cancelled: boolean }, options: { showLoading?: boolean } = {}) => {
      if (options.showLoading ?? true) {
        setMicrophoneDevicesLoaded(false);
      }
      setMicrophoneDevicesError(null);
      try {
        const devices = await listMicrophoneDevices();
        if (signal?.cancelled) return;
        setMicrophoneDevices(devices);
        setMicrophoneDevicesLoaded(true);
      } catch (err) {
        console.error('[settings] list microphone devices failed', err);
        if (signal?.cancelled) return;
        setMicrophoneDevices([]);
        setMicrophoneDevicesError(err instanceof Error ? err.message : String(err));
        setMicrophoneDevicesLoaded(true);
      }
    },
    [],
  );

  useEffect(() => {
    const signal = { cancelled: false };
    void loadMicrophoneDevices(signal);
    return () => {
      signal.cancelled = true;
    };
  }, [loadMicrophoneDevices]);

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    async function listenForDeviceChanges() {
      const { listen } = await import('@tauri-apps/api/event');
      if (cancelled) return;
      const stopListening = await listen('microphone:devices-changed', () => {
        void loadMicrophoneDevices(undefined, { showLoading: false });
      });
      if (cancelled) {
        stopListening();
        return;
      }
      unlisten = stopListening;
    }
    void listenForDeviceChanges();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [loadMicrophoneDevices]);

  useEffect(() => {
    if (!isTauri || os !== 'win') {
      setVoiceActivationModelMissing(false);
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    async function listenForVoiceActivationChanges() {
      const { listen } = await import('@tauri-apps/api/event');
      if (cancelled) return;
      const stopListening = await listen<VoiceActivationStateSnapshot>(
        'voice-activation:state-changed',
        (event) => {
          voiceActivationStateObservedRef.current = true;
          setVoiceActivationModelMissing(isVoiceActivationModelMissing(event.payload));
        },
      );
      if (cancelled) {
        stopListening();
        return;
      }
      unlisten = stopListening;
    }
    void listenForVoiceActivationChanges();
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

  const saveKeyboardListAffectingPrefs = useCallback(
    async (nextPrefs: UserPreferences) => {
      try {
        await savePrefs(nextPrefs);
      } catch (error) {
        console.error('[settings] keyboard list visibility pref save failed', error);
        emitSaved('failed', t('settings.recording.windowsShowOpenlessInKeyboardListError'));
        await refresh();
      }
    },
    [savePrefs, refresh, t],
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
  const onAudioEnhancementChange = (audioEnhancementEnabled: boolean) =>
    savePrefs({ ...prefs, audioEnhancementEnabled });
  const onEchoCancellationChange = (echoCancellationEnabled: boolean) =>
    savePrefs({ ...prefs, echoCancellationEnabled });
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
    void saveKeyboardListAffectingPrefs({
      ...prefs,
      windowsInsertionMode,
      windowsSendInputInsertionOnly: windowsInsertionMode === 'sendInput',
    });
  const onWindowsSendInputNewlineModeChange = (
    windowsSendInputNewlineMode: WindowsSendInputNewlineMode,
  ) => savePrefs({ ...prefs, windowsSendInputNewlineMode });
  const onMacosNewlineModeChange = (macosNewlineMode: MacosNewlineMode) =>
    savePrefs({ ...prefs, macosNewlineMode });
  const onWindowsShowOpenlessInKeyboardListChange = (windowsShowOpenlessInKeyboardList: boolean) =>
    void saveKeyboardListAffectingPrefs({ ...prefs, windowsShowOpenlessInKeyboardList });
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
      <Card>
        <SectionTitle hint={t('settings.recording.desc')}>
          {t('settings.recording.title')}
        </SectionTitle>
        {isHotkeyModeMigrationNoticeActive() && showDesktopHotkey && (
          <div
            style={{
              marginTop: 4,
              marginBottom: 8,
              padding: '12px 14px',
              borderRadius: 10,
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
        {platformCaps?.platform === 'desktop' && (
          <SettingRow
            label={t('settings.recording.echoCancellationLabel')}
            desc={t('settings.recording.echoCancellationDesc')}
          >
            <Toggle
              on={prefs.echoCancellationEnabled ?? false}
              onToggle={onEchoCancellationChange}
            />
          </SettingRow>
        )}
        <>
          <SettingRow
            label={t('settings.recording.audioEnhancementLabel')}
            desc={t('settings.recording.audioEnhancementDesc')}
          >
            <Toggle on={prefs.audioEnhancementEnabled} onToggle={onAudioEnhancementChange} />
          </SettingRow>
          {prefs.audioEnhancementEnabled && (
            <AudioDspSettings
              value={prefs.audioDsp}
              onChange={(audioDsp) => void savePrefs({ ...prefs, audioDsp })}
            />
          )}
        </>
        {!isAndroid && (
          <SettingRow
            label={t('settings.recording.capsuleLabel')}
            desc={t('settings.recording.capsuleDesc')}
          >
            <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
          </SettingRow>
        )}
        {!isAndroid && (os === 'mac' || os === 'win') && (
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
        {!isAndroid && (
          <SettingRow label={t('settings.recording.capsuleStyleLabel')}>
            <div style={{ minWidth: 0 }}>
              <Segmented
                options={[
                  { value: 'classic', label: t('settings.recording.capsuleStyleClassic') },
                  { value: 'siri', label: t('settings.recording.capsuleStyleSiri') },
                  {
                    value: 'siriEnhanced',
                    label: t('settings.recording.capsuleStyleSiriEnhanced'),
                  },
                ]}
                value={
                  parseCapsuleStyle(prefs.capsuleStyle) === 'siri' && prefs.capsuleSiriEnhanced
                    ? 'siriEnhanced'
                    : (parseCapsuleStyle(prefs.capsuleStyle) ?? 'classic')
                }
                // "Siri enhanced" is Siri plus a device-local flag, so synced devices that do not
                // know it keep showing plain Siri.
                onChange={(next) =>
                  savePrefs(
                    next === 'siriEnhanced'
                      ? { ...prefs, capsuleStyle: 'siri', capsuleSiriEnhanced: true }
                      : { ...prefs, capsuleStyle: next as CapsuleStyle, capsuleSiriEnhanced: false },
                  )
                }
                label={t('settings.recording.capsuleStyleLabel')}
                className="ol-capsule-style-segmented"
              />
              <CapsuleStylePreview style={parseCapsuleStyle(prefs.capsuleStyle) ?? 'classic'} />
            </div>
          </SettingRow>
        )}
        {!isAndroid && (
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
        )}
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
                  borderRadius: 10,
                  background: 'var(--ol-surface-2)',
                  border: '0.5px solid var(--ol-line)',
                }}
              >
                <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 8 }}>
                  {t('settings.recording.voiceActivationLocalOnly')}
                </div>
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
                      borderRadius: 8,
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
                      savePrefs({ ...prefs, voiceActivationWakePhrase: event.currentTarget.value })
                    }
                    style={{ ...inputStyle, width: '100%', margin: '2px 0 8px' }}
                  />
                )}
                <SettingRow label={t('settings.recording.voiceActivationFinishEnabled')}>
                  <Toggle
                    on={prefs.voiceActivationFinishEnabled ?? true}
                    onToggle={(next) => savePrefs({ ...prefs, voiceActivationFinishEnabled: next })}
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
                borderRadius: 8,
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

      {/* ─── Insertion & clipboard (collapsed; macOS / Windows only) ──────────────── */}
      {showDesktopInsert && (
        <Collapsible title={t('settings.recording.insertGroupTitle')}>
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
                // 纯文本插入模式开启时，Shift+Insert 由开关接管，这里只读展示。
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
          {capability.adapter === 'windowsLowLevel' &&
            showWindowsOpenlessKeyboardListToggle(
              prefs.windowsInsertionMode,
              prefs.windowsSendInputInsertionOnly,
            ) && (
              <SettingRow
                label={t('settings.recording.windowsShowOpenlessInKeyboardListLabel')}
                desc={t('settings.recording.windowsShowOpenlessInKeyboardListDesc')}
              >
                <Toggle
                  on={prefs.windowsShowOpenlessInKeyboardList}
                  onToggle={(next) => void onWindowsShowOpenlessInKeyboardListChange(next)}
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
          {capability.adapter === 'windowsLowLevel' && prefs.windowsInsertionMode === 'tsf' && (
            <SettingRow
              label={t('settings.advanced.windowsCompositionInputLabel')}
              desc={t('settings.advanced.windowsCompositionInputDesc')}
            >
              <Toggle
                on={!!prefs.windowsCompositionInput}
                onToggle={(next) => void savePrefs({ ...prefs, windowsCompositionInput: next })}
              />
            </SettingRow>
          )}
        </Collapsible>
      )}
      {/* ─── Startup (collapsed) ──────────────────────────────────────────── */}
      {showDesktopStartup && (
        <Collapsible title={t('settings.recording.startupGroupTitle')} defaultOpen>
          <AutostartRow />
          <SettingRow label={t('settings.recording.startMinimizedLabel')}>
            <Toggle on={prefs.startMinimized} onToggle={onStartMinimizedChange} />
          </SettingRow>
          <SettingRow label={t('settings.recording.autoUpdateCheckLabel')}>
            <Toggle on={prefs.autoUpdateCheck} onToggle={onAutoUpdateCheckChange} />
          </SettingRow>
        </Collapsible>
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
    if (!isTauri) {
      setLoaded(true);
      return;
    }
    let cancelled = false;
    autostartIsEnabled()
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
      if (!isTauri) return;
      if (next) await autostartEnable();
      else await autostartDisable();
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
