// 通用 → 录音与输入：录音快捷键 / 方式 / 麦克风 / 胶囊 / 静音，
// 外加「插入与剪贴板」「启动」两个折叠组。流式输入也并到这里（属于插入行为）。
// 自 Settings.tsx 的 RecordingSection 拆出，录音相关逻辑零改动。

import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { playRecordStartCue } from '../../lib/audioCue';
import { isHotkeyModeMigrationNoticeActive } from '../../lib/hotkeyMigration';
import {
  isTauri,
  listMicrophoneDevices,
  setDictationHotkey,
} from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import type { HotkeyMode, MicrophoneDevice, PasteShortcut, PlatformCapabilities } from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite } from '../../components/ui/SelectLite';
import { Card, Collapsible } from '../_atoms';
import { SettingRow, Toggle, inputStyle, segmentedTrackStyle } from './shared';
import { MicrophoneSelect } from './MicrophoneSelect';
import { detectOS } from '../../components/WindowChrome';

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
  const { prefs, capability, updatePrefs: savePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const [microphoneDevices, setMicrophoneDevices] = useState<MicrophoneDevice[]>([]);
  const [microphoneDevicesLoaded, setMicrophoneDevicesLoaded] = useState(false);
  const [microphoneDevicesError, setMicrophoneDevicesError] = useState<string | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const loadMicrophoneDevices = useCallback(async (
    signal?: { cancelled: boolean },
    options: { showLoading?: boolean } = {},
  ) => {
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
  }, []);

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

  if (!prefs || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const isAndroid = platformCaps?.platform === 'android';
  const showDesktopHotkey = platformCaps?.supportsDesktopHotkey === true;
  const showDesktopInsert = showDesktopHotkey && os !== 'linux';
  const showDesktopStartup = showDesktopHotkey;

  const onModeChange = (mode: HotkeyMode) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, mode } });
  const onShowCapsuleChange = (showCapsule: boolean) =>
    savePrefs({ ...prefs, showCapsule });
  const onMuteDuringRecordingChange = (muteDuringRecording: boolean) =>
    savePrefs({ ...prefs, muteDuringRecording });
  const onAudioCueChange = (audioCueOnRecord: boolean) =>
    savePrefs({ ...prefs, audioCueOnRecord });
  const onMicrophoneDeviceChange = (microphoneDeviceName: string) =>
    savePrefs({ ...prefs, microphoneDeviceName });
  const onRestoreClipboardChange = (restoreClipboardAfterPaste: boolean) =>
    savePrefs({ ...prefs, restoreClipboardAfterPaste });
  const onPasteShortcutChange = (pasteShortcut: PasteShortcut) =>
    savePrefs({ ...prefs, pasteShortcut });
  const onAllowNonTsfFallbackChange = (allowNonTsfInsertionFallback: boolean) =>
    savePrefs({ ...prefs, allowNonTsfInsertionFallback });
  const onStartMinimizedChange = (startMinimized: boolean) =>
    savePrefs({ ...prefs, startMinimized });
  const onAutoUpdateCheckChange = (autoUpdateCheck: boolean) =>
    savePrefs({ ...prefs, autoUpdateCheck });

  const choices: Array<[HotkeyMode, string]> = [
    ['toggle', t('settings.recording.modeToggle')],
    ['hold', t('settings.recording.modeHold')],
  ];
  const preferredMicrophoneAvailable = Boolean(
    prefs.microphoneDeviceName
    && microphoneDevices.some(device => device.name === prefs.microphoneDeviceName),
  );
  const effectiveMicrophoneDeviceName = prefs.microphoneDeviceName
    && (!microphoneDevicesLoaded || preferredMicrophoneAvailable)
    ? prefs.microphoneDeviceName
    : '';

  return (
    <>
      <Card>
        <div style={{ marginBottom: 6 }}>
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)', letterSpacing: '-0.01em' }}>
            {t('settings.recording.title')}
          </div>
        </div>
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
            <div style={{ fontSize: 12.5, fontWeight: 600, color: 'var(--ol-blue)', marginBottom: 4 }}>
              {t('settings.recording.migrationNoticeTitle')}
            </div>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
              {t('settings.recording.migrationNoticeDesc')}
            </div>
          </div>
        )}
        {showDesktopHotkey && (
        <SettingRow label={t('settings.recording.hotkeyLabel')}>
          <ShortcutRecorder
            value={prefs.dictationHotkey}
            onSave={async binding => {
              await setDictationHotkey(binding);
              await savePrefs({ ...prefs, dictationHotkey: binding });
            }}
          />
        </SettingRow>
        )}
        {showDesktopHotkey && (
        <SettingRow label={t('settings.recording.modeLabel')}>
          <div style={segmentedTrackStyle}>
            {choices.map(([v, l]) => (
              <button
                key={v}
                onClick={() => onModeChange(v)}
                style={{
                  padding: '5px 14px', fontSize: 12, fontWeight: 500,
                  border: 0, borderRadius: 6, fontFamily: 'inherit',
                  background: prefs.hotkey.mode === v ? 'var(--ol-segmented-active-bg)' : 'transparent',
                  color: prefs.hotkey.mode === v ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                  boxShadow: prefs.hotkey.mode === v ? 'var(--ol-segmented-active-shadow)' : 'none',
                  cursor: 'default',
                  transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
                }}
              >
                {l}
              </button>
            ))}
          </div>
        </SettingRow>
        )}
        <SettingRow label={t('settings.recording.microphoneLabel')}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            <MicrophoneSelect
              devices={microphoneDevices}
              selectedName={effectiveMicrophoneDeviceName}
              onSelect={onMicrophoneDeviceChange}
              onOpen={() => { void loadMicrophoneDevices(undefined, { showLoading: false }); }}
            />
            {microphoneDevicesError && (
              <div style={{ fontSize: 11, color: 'var(--ol-err)', lineHeight: 1.5 }}>
                {t('settings.recording.microphoneLoadError', { message: microphoneDevicesError })}
              </div>
            )}
          </div>
        </SettingRow>
        {os !== 'linux' && !isAndroid && (
        <SettingRow label={t('settings.recording.capsuleLabel')}>
          <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
        </SettingRow>
        )}
        <SettingRow label={t('settings.recording.muteDuringRecordingLabel')}>
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
        {os === 'linux' && (
        <SettingRow label={t('settings.advanced.streamingInsertLabel')}>
          <Toggle
            on={!!prefs.streamingInsert}
            onToggle={(next) => void savePrefs({ ...prefs, streamingInsert: next })}
          />
        </SettingRow>
        )}
      </Card>

      {/* ─── 插入与剪贴板（折叠，仅 macOS / Windows） ──────────────── */}
      {showDesktopInsert && (
      <Collapsible title={t('settings.recording.insertGroupTitle')}>
        <SettingRow label={t('settings.recording.restoreClipboardLabel')}>
          <Toggle on={prefs.restoreClipboardAfterPaste} onToggle={onRestoreClipboardChange} />
        </SettingRow>
        {capability.adapter !== 'macEventTap' && (
          <SettingRow label={t('settings.recording.pasteShortcutLabel')}>
            <SelectLite
              value={prefs.pasteShortcut}
              onChange={next => onPasteShortcutChange(next as PasteShortcut)}
              options={[
                { value: 'ctrlV', label: t('settings.recording.pasteShortcutCtrlV') },
                { value: 'ctrlShiftV', label: t('settings.recording.pasteShortcutCtrlShiftV') },
                { value: 'shiftInsert', label: t('settings.recording.pasteShortcutShiftInsert') },
              ]}
              ariaLabel={t('settings.recording.pasteShortcutLabel')}
              style={{ ...inputStyle, maxWidth: 220 }}
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
        {/* 流式输入：润色 SSE 一边到达一边模拟键盘逐字落到光标，降低感知延迟。
            不满足条件时自动回落一次性插入。属于「插入行为」，故归到本组。 */}
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
      {/* ─── 启动（折叠） ──────────────────────────────────────────── */}
      {showDesktopStartup && (
      <Collapsible title={t('settings.recording.startupGroupTitle')}>
        <AutostartRow />
        <SettingRow label={t('settings.recording.startMinimizedLabel')}>
          <Toggle on={prefs.startMinimized} onToggle={onStartMinimizedChange} />
        </SettingRow>
        <SettingRow label={t('settings.recording.autoUpdateCheckLabel')}>
          <Toggle on={prefs.autoUpdateCheck} onToggle={onAutoUpdateCheckChange} />
        </SettingRow>
        {capability.statusHint && (
          <div style={{ marginTop: 6, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.5 }}>
            {capability.statusHint}
          </div>
        )}
      </Collapsible>
      )}
    </>
  );
}

// 不存进 prefs：autostart 状态由 OS 持有（mac LaunchAgent plist / linux .desktop /
// windows HKCU\Run），prefs 缓存反而会与 OS 真相不一致。issue #194。
function AutostartRow() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // 切 plist / 注册表失败时给用户看的错误。null = 没有失败/上次操作已成功。
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
    <SettingRow label={t('settings.recording.startupAtBoot')}>
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
