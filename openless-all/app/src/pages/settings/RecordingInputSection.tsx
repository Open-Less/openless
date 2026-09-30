// Recording & input settings: recording mode, audio cues, input behavior, and
// platform-specific options.

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CapsuleStylePreview } from '../../components/TypelessCapsule';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { playRecordStartCue } from '../../lib/audioCue';
import { defaultDictationHotkey } from '../../lib/hotkey';
import { emitSaved } from '../../lib/savedEvent';
import { isHotkeyModeMigrationNoticeActive } from '../../lib/hotkeyMigration';
import {
  showWindowsOpenlessKeyboardListToggle,
  showWindowsSendInputNewlineMode,
} from '../../lib/windowsKeyboardListToggle';
import { isTauri, listMicrophoneDevices, setDictationHotkey } from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
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
import { Card, Collapsible } from '../_atoms';
import { SectionTitle, SettingRow, Toggle, inputStyle, segmentedTrackStyle } from './shared';
import { MicrophoneSelect } from './MicrophoneSelect';
import { detectOS } from '../../components/WindowChrome';

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

export function RecordingInputSection() {
  const { t } = useTranslation();
  const os = detectOS();
  const { prefs, capability, updatePrefs: savePrefs, refresh } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const [microphoneDevices, setMicrophoneDevices] = useState<MicrophoneDevice[]>([]);
  const [microphoneDevicesLoaded, setMicrophoneDevicesLoaded] = useState(false);
  const [microphoneDevicesError, setMicrophoneDevicesError] = useState<string | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  // 兼容旧 Windows 配置：Shift+Insert 仍保留在跨平台类型/后端中，
  // 但 Windows 已不再提供该选项；进入设置时迁移为 Ctrl+V，避免下拉框无匹配值。
  useEffect(() => {
    if (os !== 'win' || prefs?.pasteShortcut !== 'shiftInsert') return;
    void savePrefs((current) => {
      if (current.pasteShortcut !== 'shiftInsert') return current;
      return { ...current, pasteShortcut: 'ctrlV' };
    }).catch((error) => {
      console.warn('[settings] migrate Windows paste shortcut failed', error);
    });
  }, [os, prefs?.pasteShortcut, savePrefs]);

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

  if (!prefs || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const isAndroid = platformCaps?.platform === 'android';
  const showDesktopHotkey = platformCaps?.supportsDesktopHotkey === true;
  const showDesktopInsert = showDesktopHotkey;
  const showDesktopStartup = showDesktopHotkey;
  const effectivePasteShortcut =
    os === 'win' && prefs.pasteShortcut === 'shiftInsert' ? 'ctrlV' : prefs.pasteShortcut;

  const onModeChange = (mode: HotkeyMode) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, mode } });
  const onShowCapsuleChange = (showCapsule: boolean) => savePrefs({ ...prefs, showCapsule });
  const onMuteDuringRecordingChange = (muteDuringRecording: boolean) =>
    savePrefs({ ...prefs, muteDuringRecording });
  const onAudioCueChange = (audioCueOnRecord: boolean) => savePrefs({ ...prefs, audioCueOnRecord });
  const onMicrophoneDeviceChange = (microphoneDeviceName: string) =>
    savePrefs({ ...prefs, microphoneDeviceName });
  const onRestoreClipboardChange = (restoreClipboardAfterPaste: boolean) =>
    savePrefs({ ...prefs, restoreClipboardAfterPaste });
  const onPasteShortcutChange = (pasteShortcut: PasteShortcut) =>
    savePrefs({ ...prefs, pasteShortcut });
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

  // Sliding thumb of the horizontal segmented control for the recording mode
  // (hold-to-talk / auto, etc.): follows the selected item, and the left/width transition
  // is the switch animation. The button's offsetParent is the track (position:relative),
  // and offsetLeft shares the containing block origin of the absolutely positioned thumb
  // (the track's padding edge), so assign directly; useLayoutEffect positions before
  // paint, so no first-frame flash.
  // The dependency list must include showDesktopHotkey: prefs and platformCaps load
  // asynchronously, so on first entry there can be a timing where activeMode is set but
  // the buttons aren't mounted yet (prefs before caps) and the effect returns early; only
  // after the buttons mount does this effect run again — without this dependency the thumb
  // would never be positioned (user report: the selected recording mode wasn't shown on
  // first visit to settings).
  const modeTrackRef = useRef<HTMLDivElement | null>(null);
  const modeButtonsRef = useRef(new Map<HotkeyMode, HTMLButtonElement>());
  const [modeThumb, setModeThumb] = useState<{ left: number; width: number } | null>(null);
  const activeMode = prefs?.hotkey.mode;
  useLayoutEffect(() => {
    const track = modeTrackRef.current;
    if (!track) return;
    const measure = () => {
      const active = activeMode ? modeButtonsRef.current.get(activeMode) : undefined;
      if (!active) return;
      setModeThumb({ left: active.offsetLeft, width: active.offsetWidth });
    };
    measure();
    // Language switches (button text reflow) and window resizes both change button sizes;
    // the ResizeObserver re-measures as a safety net so the thumb never stays at a stale
    // position (pr_agent #912 feedback).
    const observer = new ResizeObserver(measure);
    observer.observe(track);
    return () => observer.disconnect();
  }, [activeMode, showDesktopHotkey]);

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
              style={{ fontSize: 12.5, fontWeight: 600, color: 'var(--ol-blue)', marginBottom: 4 }}
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
              allowMacDictationKey={os === 'mac'}
              allowMouseButtons={os === 'win'}
              // 录音快捷键是核心热键，Rust 端不接受 null，不可停用——置灰并提示。
              disableDisabled
              disableHint={t('settings.recording.comboDisableHint')}
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
            <div ref={modeTrackRef} style={{ ...segmentedTrackStyle, position: 'relative' }}>
              {/* Sliding thumb: carries the selected-state background/shadow and transitions
                left/width smoothly on switch; the buttons only change text color. Not
                rendered before measuring (before the first-frame layout effect); positioned
                before paint, so no flash. */}
              {modeThumb && (
                <div
                  style={{
                    position: 'absolute',
                    top: 2,
                    bottom: 2,
                    left: modeThumb.left,
                    width: modeThumb.width,
                    borderRadius: 6,
                    background: 'var(--ol-segmented-active-bg)',
                    boxShadow: 'var(--ol-segmented-active-shadow)',
                    transition:
                      'left 0.18s var(--ol-motion-soft), width 0.18s var(--ol-motion-soft)',
                    pointerEvents: 'none',
                  }}
                />
              )}
              {choices.map(([v, l]) => (
                <button
                  key={v}
                  ref={(el) => {
                    if (el) modeButtonsRef.current.set(v, el);
                  }}
                  onClick={() => onModeChange(v)}
                  style={{
                    padding: '5px 14px',
                    fontSize: 12,
                    fontWeight: 500,
                    border: 0,
                    borderRadius: 6,
                    fontFamily: 'inherit',
                    position: 'relative',
                    zIndex: 1,
                    background: 'transparent',
                    color: prefs.hotkey.mode === v ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                    cursor: 'default',
                    transition: 'color 0.16s var(--ol-motion-quick)',
                  }}
                >
                  {l}
                </button>
              ))}
            </div>
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
        {!isAndroid && (
          <SettingRow
            label={t('settings.recording.capsuleLabel')}
            desc={t('settings.recording.capsuleDesc')}
          >
            <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
          </SettingRow>
        )}
        {!isAndroid && (
          <SettingRow label={t('settings.recording.capsuleStyleLabel')}>
            <div style={{ minWidth: 0 }}>
              <SelectLite
                value={prefs.capsuleStyle ?? 'siri'}
                onChange={(next) => savePrefs({ ...prefs, capsuleStyle: next as CapsuleStyle })}
                options={[
                  { value: 'siri', label: t('settings.recording.capsuleStyleSiri') },
                  { value: 'classic', label: t('settings.recording.capsuleStyleClassic') },
                  { value: 'typeless', label: t('settings.recording.capsuleStyleTypeless') },
                ]}
                ariaLabel={t('settings.recording.capsuleStyleLabel')}
                style={{ maxWidth: 220, minWidth: 200 }}
              />
              <CapsuleStylePreview style={prefs.capsuleStyle ?? 'siri'} />
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
                fontWeight: 500,
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
                value={effectivePasteShortcut}
                onChange={(next) => onPasteShortcutChange(next as PasteShortcut)}
                options={[
                  { value: 'ctrlV', label: t('settings.recording.pasteShortcutCtrlV') },
                  { value: 'ctrlShiftV', label: t('settings.recording.pasteShortcutCtrlShiftV') },
                  // 这个「粘贴与剪贴板」组只在 Windows 出现（mac 走
                  // macEventTap 不显示本行）。Shift+Insert 是 xterm/urxvt 等 X11 终端的粘贴组合，
                  // 放在 Windows 上纯属误导，故不再作为选项（issue #786）。
                ]}
                ariaLabel={t('settings.recording.pasteShortcutLabel')}
                style={{ ...inputStyle, maxWidth: 220 }}
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

// 不存进 prefs：autostart 状态由 OS 持有（mac LaunchAgent plist /
// windows HKCU\Run），prefs 缓存反而会与 OS 真相不一致。issue #194。
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
