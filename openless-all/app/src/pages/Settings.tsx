// Settings.tsx — ported verbatim from design_handoff_openless/pages.jsx::Settings.
// Internal sub-sections (Recording / Providers / Shortcuts / Permissions / Language / About)
// keep their inline-style literals 1:1 with the source JSX.

import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { ShortcutRecorder } from '../components/ShortcutRecorder';
import { isDialogStatus, UpdateDialog, useAutoUpdate } from '../components/AutoUpdate';
import { detectOS } from '../components/WindowChrome';
import { APP_VERSION_LABEL } from '../lib/appVersion';
import { isHotkeyModeMigrationNoticeActive } from '../lib/hotkeyMigration';
import {
  defaultQaShortcut,
  getHotkeyBindingCodes,
  getHotkeyBindingLabel,
  getHotkeyCodeLabel,
} from '../lib/hotkey';
import { createHotkeyRecorderState, orderHotkeyCodes, updateHotkeyRecorderState } from '../lib/hotkeyRecorder';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getHotkeyStatus,
  getWindowsImeStatus,
  isTauri,
  listMicrophoneDevices,
  openExternal,
  openSystemSettings,
  listProviderModels,
  readCredential,
  requestAccessibilityPermission,
  requestMicrophonePermission,
  setActiveAsrProvider,
  setActiveLlmProvider,
  setCredential,
  setDictationHotkey,
  setOpenAppHotkey,
  setQaHotkey,
  setSwitchStyleHotkey,
  setTranslationHotkey,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
  validateProviderCredentials,
} from '../lib/ipc';
import type {
  HotkeyCapability,
  HotkeyBinding,
  HotkeyMode,
  HotkeyStatus,
  HotkeyTrigger,
  MicrophoneDevice,
  PermissionStatus,
  WindowsImeStatus,
} from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import i18n, {
  FOLLOW_SYSTEM,
  getLocalePreference,
  outputPrefsForLocale,
  setLocalePreference,
  type SupportedLocale,
} from '../i18n';
import { Btn, Card, PageHeader, Pill } from './_atoms';
import {
  deleteLocalAsrModel,
  getLocalAsrSettings,
  listLocalAsrModels,
  type LocalAsrModelStatus,
  type LocalAsrSettings,
} from '../lib/localAsr';

/// Settings → ASR 选了 local-qwen3 时触发跳到「模型设置」页 + 关 Settings modal。
/// FloatingShell 监听同名事件做 setCurrentTab('localAsr') + setSettingsOpen(false)。
export const NAVIGATE_LOCAL_ASR_EVENT = 'openless:navigate-local-asr';

interface SettingsProps {
  embedded?: boolean;
  initialSection?: SettingsSectionId;
}
// "关于" tab 已移除（内容并入外层 SettingsModal 的 About 页，避免设置内外重复入口）。
export type SettingsSectionId = 'recording' | 'providers' | 'shortcuts' | 'permissions' | 'language';

const SECTION_ORDER: SettingsSectionId[] = ['recording', 'providers', 'shortcuts', 'permissions', 'language'];

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

export function Settings({ embedded = false, initialSection = 'recording' }: SettingsProps) {
  const { t } = useTranslation();
  const [section, setSection] = useState<SettingsSectionId>(initialSection);

  useEffect(() => {
    setSection(initialSection);
  }, [initialSection]);

  return (
    <>
      {!embedded && (
        <PageHeader
          kicker={t('settings.kicker')}
          title={t('settings.title')}
          desc={t('settings.desc')}
        />
      )}
      {/* embedded（在 SettingsModal 里）模式下：mini-sidebar 固定，仅右栏 scroll。
          外层 flex:1 minHeight:0 让 grid 拿到确定高度；gridTemplateRows: minmax(0, 1fr)
          强制行高等于容器高度，否则 grid 默认 auto rows 会跟内容长，右栏 overflow:auto
          就退化成"没东西需要 scroll"，于是大家照旧一起飘。 */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: embedded ? '120px 1fr' : '160px 1fr',
          gap: 18,
          ...(embedded ? { flex: 1, minHeight: 0, gridTemplateRows: 'minmax(0, 1fr)' } : {}),
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {SECTION_ORDER.map(s => (
            <button
              key={s}
              onClick={() => setSection(s)}
              style={{
                padding: '8px 12px', textAlign: 'left',
                fontSize: 13, color: section === s ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                background: section === s ? 'rgba(0,0,0,0.04)' : 'transparent',
                border: 0, borderRadius: 8, fontFamily: 'inherit', fontWeight: section === s ? 600 : 500,
                cursor: 'default',
                transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
              }}
            >
              {t(`settings.sections.${s}`)}
            </button>
          ))}
        </div>
        <div
          className={embedded ? 'ol-thinscroll' : undefined}
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
            ...(embedded ? { minHeight: 0, overflow: 'auto', paddingRight: 4 } : {}),
          }}
        >
          {section === 'recording' && <RecordingSection />}
          {section === 'providers' && <ProvidersSection />}
          {section === 'shortcuts' && <ShortcutsSection />}
          {section === 'permissions' && <PermissionsSection />}
          {section === 'language' && <LanguageSection />}
        </div>
      </div>
    </>
  );
}

interface SettingRowProps {
  label: string;
  desc?: string;
  children: ReactNode;
  controlWidth?: number | string;
}

function SettingRow({ label, desc, children, controlWidth }: SettingRowProps) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 180px) minmax(0, 1fr)', gap: 16, padding: '14px 0', borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</div>
        {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>{desc}</div>}
      </div>
      <div style={{ display: 'flex', alignItems: 'flex-start', minWidth: 0, width: controlWidth ?? 'auto' }}>{children}</div>
    </div>
  );
}

function RecordingSection() {
  const { t } = useTranslation();
  const { prefs, capability, updatePrefs: savePrefs } = useHotkeySettings();
  const [microphoneDevices, setMicrophoneDevices] = useState<MicrophoneDevice[]>([]);
  const [microphoneDevicesLoaded, setMicrophoneDevicesLoaded] = useState(false);
  const [microphoneDevicesError, setMicrophoneDevicesError] = useState<string | null>(null);
  const [microphonePickerOpen, setMicrophonePickerOpen] = useState(false);

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

  useEffect(() => {
    if (microphonePickerOpen) {
      void loadMicrophoneDevices(undefined, { showLoading: false });
    }
  }, [loadMicrophoneDevices, microphonePickerOpen]);

  if (!prefs || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const onModeChange = (mode: HotkeyMode) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, mode } });
  const onShowCapsuleChange = (showCapsule: boolean) =>
    savePrefs({ ...prefs, showCapsule });
  const onMuteDuringRecordingChange = (muteDuringRecording: boolean) =>
    savePrefs({ ...prefs, muteDuringRecording });
  const onMicrophoneDeviceChange = (microphoneDeviceName: string) =>
    savePrefs({ ...prefs, microphoneDeviceName });
  const onRestoreClipboardChange = (restoreClipboardAfterPaste: boolean) =>
    savePrefs({ ...prefs, restoreClipboardAfterPaste });
  const onAllowNonTsfFallbackChange = (allowNonTsfInsertionFallback: boolean) =>
    savePrefs({ ...prefs, allowNonTsfInsertionFallback });

  const choices: Array<[HotkeyMode, string]> = [
    ['toggle', t('settings.recording.modeToggle')],
    ['hold', t('settings.recording.modeHold')],
  ];
  const hotkeyDesc = capability.requiresAccessibilityPermission
    ? t('settings.recording.hotkeyDescAcc')
    : t('settings.recording.hotkeyDescNoAcc');
  const preferredMicrophoneAvailable = Boolean(
    prefs.microphoneDeviceName
    && microphoneDevices.some(device => device.name === prefs.microphoneDeviceName),
  );
  const effectiveMicrophoneDeviceName = prefs.microphoneDeviceName
    && (!microphoneDevicesLoaded || preferredMicrophoneAvailable)
    ? prefs.microphoneDeviceName
    : '';
  const selectedMicrophoneLabel = effectiveMicrophoneDeviceName
    ? effectiveMicrophoneDeviceName
    : t('settings.recording.microphoneDefault');

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.recording.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>{t('settings.recording.desc')}</div>
      {isHotkeyModeMigrationNoticeActive() && (
        <div
          style={{
            marginTop: 10,
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
      <SettingRow label={t('settings.recording.hotkeyLabel')} desc={hotkeyDesc}>
        <ShortcutRecorder
          value={prefs.dictationHotkey}
          onSave={async binding => {
            await setDictationHotkey(binding);
            await savePrefs({ ...prefs, dictationHotkey: binding });
          }}
        />
      </SettingRow>
      <SettingRow label={t('settings.recording.modeLabel')} desc={t('settings.recording.modeDesc')}>
        <div style={{ display: 'inline-flex', padding: 2, borderRadius: 8, background: 'rgba(0,0,0,0.05)' }}>
          {choices.map(([v, l]) => (
            <button
              key={v}
              onClick={() => onModeChange(v)}
              style={{
                padding: '5px 14px', fontSize: 12, fontWeight: 500,
                border: 0, borderRadius: 6, fontFamily: 'inherit',
                background: prefs.hotkey.mode === v ? '#fff' : 'transparent',
                color: prefs.hotkey.mode === v ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                boxShadow: prefs.hotkey.mode === v ? '0 1px 2px rgba(0,0,0,.08)' : 'none',
                cursor: 'default',
                transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
              }}
            >
              {l}
            </button>
          ))}
        </div>
      </SettingRow>
      <SettingRow label={t('settings.recording.microphoneLabel')} desc={t('settings.recording.microphoneDesc')}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <button
            type="button"
            aria-label={t('settings.recording.microphoneLabel')}
            onClick={() => {
              setMicrophonePickerOpen(true);
            }}
            onKeyDown={e => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                setMicrophonePickerOpen(true);
              }
            }}
            onChange={() => {}}
            style={{
              ...inputStyle,
              flex: '0 0 auto',
              width: 200,
              maxWidth: 200,
              height: 32,
              minWidth: 0,
              alignSelf: 'flex-start',
              padding: '0 9px 0 10px',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 8,
              textAlign: 'left',
              color: 'var(--ol-ink)',
            }}
          >
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {selectedMicrophoneLabel}
            </span>
            <Icon name="chevRight" size={13} />
          </button>
          {!microphoneDevicesLoaded && (
            <div style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
          )}
          {microphoneDevicesError && (
            <div style={{ fontSize: 11, color: 'var(--ol-err)', lineHeight: 1.5 }}>
              {t('settings.recording.microphoneLoadError', { message: microphoneDevicesError })}
            </div>
          )}
        </div>
      </SettingRow>
      {microphonePickerOpen && (
        <MicrophonePickerDialog
          devices={microphoneDevices}
          selectedName={effectiveMicrophoneDeviceName}
          onClose={() => setMicrophonePickerOpen(false)}
          onRefresh={() => {
            void loadMicrophoneDevices();
          }}
          loading={!microphoneDevicesLoaded}
          onSelect={(name) => {
            onMicrophoneDeviceChange(name);
          }}
        />
      )}
      <SettingRow label={t('settings.recording.capsuleLabel')} desc={t('settings.recording.capsuleDesc')}>
        <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
      </SettingRow>
      <SettingRow
        label={t('settings.recording.muteDuringRecordingLabel')}
        desc={t('settings.recording.muteDuringRecordingDesc')}
      >
        <Toggle on={prefs.muteDuringRecording} onToggle={onMuteDuringRecordingChange} />
      </SettingRow>
      <SettingRow
        label={t('settings.recording.restoreClipboardLabel')}
        desc={t('settings.recording.restoreClipboardDesc')}
      >
        <Toggle on={prefs.restoreClipboardAfterPaste} onToggle={onRestoreClipboardChange} />
      </SettingRow>
      {capability.adapter === 'windowsLowLevel' && (
        <SettingRow
          label={t('settings.recording.allowNonTsfFallbackLabel')}
          desc={t('settings.recording.allowNonTsfFallbackDesc')}
        >
          <Toggle
            on={prefs.allowNonTsfInsertionFallback}
            onToggle={onAllowNonTsfFallbackChange}
          />
        </SettingRow>
      )}
      <AutostartRow />
      {capability.statusHint && (
        <div style={{ marginTop: 6, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.5 }}>
          {capability.statusHint}
        </div>
      )}
    </Card>
  );
}

function HotkeyRecorder({
  binding,
  onCommit,
}: {
  binding: HotkeyBinding;
  onCommit: (codes: string[]) => void;
}) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [draftCodes, setDraftCodes] = useState<string[]>([]);
  const recorderStateRef = useRef(createHotkeyRecorderState());
  const recordingRef = useRef(false);

  const resetRecording = () => {
    recordingRef.current = false;
    recorderStateRef.current = createHotkeyRecorderState();
    setDraftCodes([]);
    setRecording(false);
  };

  const commitCodes = (codes: string[]) => {
    const ordered = orderHotkeyCodes(codes);
    resetRecording();
    onCommit(ordered);
  };

  const startRecording = () => {
    recordingRef.current = true;
    recorderStateRef.current = createHotkeyRecorderState();
    setDraftCodes([]);
    setRecording(true);
  };

  useEffect(() => {
    if (!recording) return undefined;

    const stopEvent = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
    };

    const applyHotkeyCode = (code: string, pressed: boolean) => {
      if (!recordingRef.current) return;
      const next = updateHotkeyRecorderState(recorderStateRef.current, code, pressed);
      recorderStateRef.current = next.state;
      setDraftCodes(next.state.draftCodes);
      if (next.commitCodes) commitCodes(next.commitCodes);
    };

    const onKeyDown = (event: KeyboardEvent) => {
      stopEvent(event);
      if (event.key === 'Escape' || event.code === 'Escape') {
        resetRecording();
        return;
      }
      const code = normalizeKeyboardHotkeyCode(event);
      if (!code) return;
      applyHotkeyCode(code, true);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      stopEvent(event);
      if (!recordingRef.current) return;
      if (event.key === 'Escape' || event.code === 'Escape') {
        resetRecording();
        return;
      }
      const code = normalizeKeyboardHotkeyCode(event);
      if (!code) return;
      applyHotkeyCode(code, false);
    };

    const onMouseDown = (event: MouseEvent) => {
      const code = mouseButtonToHotkeyCode(event.button);
      if (!code) return;
      stopEvent(event);
      applyHotkeyCode(code, true);
    };

    const onMouseUp = (event: MouseEvent) => {
      const code = mouseButtonToHotkeyCode(event.button);
      if (!code) return;
      stopEvent(event);
      applyHotkeyCode(code, false);
    };

    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    window.addEventListener('mousedown', onMouseDown, true);
    window.addEventListener('mouseup', onMouseUp, true);
    return () => {
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
      window.removeEventListener('mousedown', onMouseDown, true);
      window.removeEventListener('mouseup', onMouseUp, true);
    };
  }, [recording]);

  const label = recording
    ? draftCodes.length > 0
      ? draftCodes.map(getHotkeyCodeLabel).join('+')
      : t('settings.recording.hotkeyRecording')
    : getHotkeyBindingLabel(binding);
  const hasKeys = getHotkeyBindingCodes(binding).length > 0;

  return (
    <div style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
      <button
        type="button"
        onClick={startRecording}
        style={{
          ...hotkeyRecorderButtonStyle,
          borderColor: recording ? 'var(--ol-blue)' : 'var(--ol-line-strong)',
          color: recording ? 'var(--ol-blue)' : 'var(--ol-ink)',
        }}
      >
        <span style={hotkeyRecorderLabelStyle}>{label}</span>
        {!recording && hasKeys && (
          <span
            role="button"
            tabIndex={0}
            aria-label={t('settings.recording.hotkeyClear')}
            onClick={event => {
              event.stopPropagation();
              onCommit([]);
            }}
            onKeyDown={event => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                event.stopPropagation();
                onCommit([]);
              }
            }}
            style={hotkeyClearButtonStyle}
          >
            <Icon name="x" size={11} strokeWidth={2} />
          </span>
        )}
      </button>
    </div>
  );
}

function MicrophonePickerDialog({
  devices,
  selectedName,
  onClose,
  onRefresh,
  loading,
  onSelect,
}: {
  devices: MicrophoneDevice[];
  selectedName: string;
  onClose: () => void;
  onRefresh: () => void;
  loading: boolean;
  onSelect: (name: string) => void;
}) {
  const { t } = useTranslation();
  const [pickedName, setPickedName] = useState(selectedName);
  const [previewName, setPreviewName] = useState(selectedName);
  const [level, setLevel] = useState(0);
  const [hoveredName, setHoveredName] = useState<string | null>(null);
  const [pressedName, setPressedName] = useState<string | null>(null);
  const [monitorError, setMonitorError] = useState<string | null>(null);
  const monitorQueueRef = useRef<Promise<void>>(Promise.resolve());

  const enqueueMonitorTask = useCallback((task: () => Promise<void>) => {
    const next = monitorQueueRef.current.catch(() => undefined).then(task);
    monitorQueueRef.current = next.catch(() => undefined);
    return next;
  }, []);

  useEffect(() => {
    setPickedName(selectedName);
    setPreviewName(selectedName);
  }, [selectedName]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let timer: number | undefined;
    setLevel(0);
    setMonitorError(null);

    async function start() {
      await enqueueMonitorTask(async () => {
        try {
          if (isTauri) {
            const { listen } = await import('@tauri-apps/api/event');
            if (cancelled) return;
            const stopListening = await listen<{ level: number }>('microphone:level', event => {
              setLevel(Math.max(0, Math.min(1, event.payload.level ?? 0)));
            });
            if (cancelled) {
              stopListening();
              return;
            }
            unlisten = stopListening;
            await startMicrophoneLevelMonitor(previewName);
            if (cancelled) {
              unlisten?.();
              unlisten = undefined;
              await stopMicrophoneLevelMonitor();
            }
          } else {
            const tick = window.setInterval(() => {
              setLevel(0.25 + Math.random() * 0.55);
            }, 120);
            if (cancelled) {
              window.clearInterval(tick);
              return;
            }
            unlisten = () => window.clearInterval(tick);
          }
        } catch (err) {
          console.warn('[settings] microphone level monitor failed', err);
          if (!cancelled) {
            setMonitorError(err instanceof Error ? err.message : String(err));
          }
        }
      });
    }

    timer = window.setTimeout(() => {
      void start();
    }, 140);
    return () => {
      cancelled = true;
      if (timer !== undefined) {
        window.clearTimeout(timer);
      }
      void enqueueMonitorTask(async () => {
        unlisten?.();
        unlisten = undefined;
        await stopMicrophoneLevelMonitor();
      });
    };
  }, [enqueueMonitorTask, previewName]);

  const rows = [
    {
      id: 'default',
      name: '',
      label: t('settings.recording.microphoneDefault'),
      desc: t('settings.recording.microphoneDefaultDesc'),
      isDefault: false,
    },
    ...devices.map((device, index) => ({
      id: `${device.name}-${index}`,
      name: device.name,
      label: device.name,
      desc: device.isDefault ? t('settings.recording.microphoneSystemDefault') : '',
      isDefault: device.isDefault,
    })),
  ];

  return (
    <div
      role="presentation"
      onClick={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 40,
        display: 'grid',
        placeItems: 'center',
        background: 'rgba(0,0,0,0.32)',
        animation: 'olMicPickerFadeIn 120ms ease-out',
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        onClick={e => e.stopPropagation()}
        style={{
          width: 450,
          maxWidth: 'calc(100vw - 48px)',
          borderRadius: 16,
          background: 'rgba(255,255,255,0.96)',
          border: '0.5px solid rgba(0,0,0,0.12)',
          boxShadow: '0 24px 70px rgba(0,0,0,0.28)',
          padding: 24,
          animation: 'olMicPickerPopIn 160ms cubic-bezier(.2,.8,.2,1)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, marginBottom: 10 }}>
          <div style={{ fontSize: 18, fontWeight: 650 }}>{t('settings.recording.microphoneDialogTitle')}</div>
          <div style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
            <button
              type="button"
              onClick={onRefresh}
              disabled={loading}
              style={{
                border: 0,
                borderRadius: 999,
                background: 'transparent',
                color: loading ? 'var(--ol-ink-4)' : 'var(--ol-ink-3)',
                cursor: 'default',
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                width: 28,
                height: 28,
                opacity: loading ? 0.65 : 1,
                transition: 'background 0.16s var(--ol-motion-quick), opacity 0.16s var(--ol-motion-quick)',
              }}
              onMouseEnter={e => {
                if (!loading) e.currentTarget.style.background = 'rgba(0,0,0,0.05)';
              }}
              onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
              title={t('common.refresh')}
            >
              <Icon
                name="refresh"
                size={14}
                style={{ animation: loading ? 'olMicPickerSpin 800ms linear infinite' : undefined }}
              />
            </button>
            <button
              type="button"
              onClick={onClose}
              style={{
                border: 0,
                borderRadius: 999,
                background: 'transparent',
                color: 'var(--ol-ink-3)',
                cursor: 'default',
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                width: 28,
                height: 28,
                transition: 'background 0.16s var(--ol-motion-quick)',
              }}
              onMouseEnter={e => (e.currentTarget.style.background = 'rgba(0,0,0,0.05)')}
              onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
              title={t('common.close')}
            >
              <Icon name="close" size={14} />
            </button>
          </div>
        </div>
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55, marginBottom: 18 }}>
          {t('settings.recording.microphoneDialogDesc')}
        </div>
        {monitorError && (
          <div style={{ fontSize: 11.5, color: 'var(--ol-err)', lineHeight: 1.45, marginBottom: 12 }}>
            {t('settings.recording.microphoneMonitorError', { message: monitorError })}
          </div>
        )}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {rows.map(row => {
            const active = pickedName === row.name;
            const previewing = previewName === row.name;
            const hovered = hoveredName === row.name;
            const pressed = pressedName === row.name;
            return (
              <button
                key={row.id}
                type="button"
                onMouseEnter={() => {
                  setHoveredName(row.name);
                }}
                onMouseLeave={() => {
                  setHoveredName(null);
                  setPressedName(null);
                }}
                onMouseDown={() => setPressedName(row.name)}
                onMouseUp={() => setPressedName(null)}
                onFocus={() => {
                  setHoveredName(row.name);
                }}
                onBlur={() => setHoveredName(null)}
                onClick={() => {
                  setPickedName(row.name);
                  setPreviewName(row.name);
                  onSelect(row.name);
                }}
                style={{
                  display: 'grid',
                  gridTemplateColumns: '1fr auto',
                  gap: 14,
                  alignItems: 'center',
                  width: '100%',
                  padding: '14px 16px',
                  borderRadius: 10,
                  border: active ? '1px solid rgba(37,99,235,0.7)' : '0.5px solid rgba(0,0,0,0.12)',
                  background: active
                    ? 'rgba(37,99,235,0.08)'
                    : hovered
                      ? 'rgba(0,0,0,0.035)'
                      : '#fff',
                  boxShadow: active
                    ? '0 0 0 3px rgba(37,99,235,0.08)'
                    : hovered
                      ? '0 8px 18px rgba(0,0,0,0.06)'
                      : '0 1px 2px rgba(0,0,0,0.03)',
                  color: 'var(--ol-ink)',
                  cursor: 'default',
                  textAlign: 'left',
                  transform: pressed ? 'scale(0.992)' : hovered ? 'translateY(-1px)' : 'translateY(0)',
                  transition: 'background 140ms ease, border-color 140ms ease, box-shadow 160ms ease, transform 120ms ease',
                }}
              >
                <span style={{ minWidth: 0 }}>
                  <span style={{ display: 'block', fontSize: 13, fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {row.label}
                  </span>
                  {row.desc && (
                    <span style={{ display: 'block', fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 3 }}>
                      {row.desc}
                    </span>
                  )}
                </span>
                <LevelMeter level={previewing ? level : 0} />
              </button>
            );
          })}
        </div>
        <style>
          {`
            @keyframes olMicPickerFadeIn {
              from { opacity: 0; }
              to { opacity: 1; }
            }
            @keyframes olMicPickerPopIn {
              from { opacity: 0; transform: translateY(8px) scale(.985); }
              to { opacity: 1; transform: translateY(0) scale(1); }
            }
            @keyframes olMicPickerSpin {
              from { transform: rotate(0deg); }
              to { transform: rotate(360deg); }
            }
          `}
        </style>
      </div>
    </div>
  );
}

function inferLegacyTrigger(codes: string[], fallback: HotkeyTrigger): HotkeyTrigger {
  if (codes.includes('ControlRight')) return 'rightControl';
  if (codes.includes('ControlLeft')) return 'leftControl';
  if (codes.includes('AltRight')) return 'rightAlt';
  if (codes.includes('AltLeft')) return 'leftOption';
  if (codes.includes('MetaRight')) return 'rightCommand';
  if (codes.includes('Fn')) return 'fn';
  return fallback;
}

function normalizeKeyboardHotkeyCode(event: KeyboardEvent): string | null {
  if (event.key === 'Fn' || event.code === 'Fn') return 'Fn';
  if (event.key === 'FnLock' || event.code === 'FnLock') return 'FnLock';
  const code = event.code === 'OSLeft' ? 'MetaLeft' : event.code === 'OSRight' ? 'MetaRight' : event.code;
  if (SUPPORTED_HOTKEY_CODES.has(code)) return code;
  if (/^Key[A-Z]$/.test(code)) return code;
  if (/^Digit[0-9]$/.test(code)) return code;
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return code;
  return null;
}

function mouseButtonToHotkeyCode(button: number): string | null {
  if (button === 3) return 'Mouse4';
  if (button === 4) return 'Mouse5';
  return null;
}

const SUPPORTED_HOTKEY_CODES = new Set([
  'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'ShiftLeft', 'ShiftRight',
  'MetaLeft', 'MetaRight', 'CapsLock', 'ScrollLock', 'Pause', 'PrintScreen',
  'Backspace', 'Tab', 'Enter', 'Space', 'Insert', 'Delete', 'Home', 'End',
  'PageUp', 'PageDown', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight',
  'ContextMenu', 'NumpadAdd', 'NumpadSubtract', 'NumpadMultiply', 'NumpadDivide',
  'NumpadDecimal', 'NumpadEnter', 'Backquote', 'Minus', 'Equal', 'BracketLeft',
  'BracketRight', 'Backslash', 'Semicolon', 'Quote', 'Comma', 'Period', 'Slash',
  'Fn', 'FnLock',
]);

function LevelMeter({ level }: { level: number }) {
  const amplified = Math.min(1, Math.max(0, level * 4.5));
  const bars = [0.25, 0.5, 0.75, 1, 0.75, 0.5];
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4, height: 32 }}>
      {bars.map((weight, index) => {
        const intensity = Math.min(1, amplified * (0.85 + weight * 0.35));
        const height = 6 + intensity * (20 * weight);
        return (
          <span
            key={`${weight}-${index}`}
            style={{
              width: 5,
              height,
              borderRadius: 999,
              background: intensity > 0.08 ? 'var(--ol-blue)' : 'rgba(0,0,0,0.10)',
              opacity: 0.35 + intensity * 0.65,
              transition: 'height 70ms linear, opacity 90ms ease, background 120ms ease',
            }}
          />
        );
      })}
    </span>
  );
}

// 不存进 prefs：autostart 状态由 OS 持有（mac LaunchAgent plist / linux .desktop /
// windows HKCU\Run），prefs 缓存反而会与 OS 真相不一致。issue #194。
function AutostartRow() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // 切 plist / 注册表失败时给用户看的错误。null = 没有失败/上次操作已成功。
  // 不渲染等于把失败吞掉 —— Windows 写 HKCU\Run 被组策略拦、macOS 写
  // LaunchAgent plist 权限不够 都是真实可能。issue #194。
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

export function Toggle({ on, onToggle }: { on: boolean; onToggle?: (next: boolean) => void }) {
  return (
    <button
      onClick={() => onToggle?.(!on)}
      style={{
        position: 'relative', width: 32, height: 18, borderRadius: 999, border: 0,
        background: on ? 'var(--ol-blue)' : 'rgba(0,0,0,0.15)',
        cursor: 'default',
        transition: 'background 0.16s var(--ol-motion-quick)',
      }}
    >
      <span
        style={{
          position: 'absolute', top: 2, left: on ? 16 : 2,
          width: 14, height: 14, borderRadius: 999, background: '#fff',
          boxShadow: '0 1px 2px rgba(0,0,0,.25)', transition: 'left .16s var(--ol-motion-spring)',
        }}
      />
    </button>
  );
}

const LLM_PRESETS = [
  {
    id: 'ark',
    nameKey: 'ark',
    baseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
    modelPlaceholder: 'deepseek-v3-2',
  },
  {
    id: 'deepseek',
    nameKey: 'deepseek',
    baseUrl: 'https://api.deepseek.com/v1',
    modelPlaceholder: 'deepseek-v4-flash',
  },
  {
    id: 'siliconflow',
    nameKey: 'siliconflow',
    baseUrl: 'https://api.siliconflow.cn/v1',
    modelPlaceholder: 'Qwen/Qwen2.5-7B-Instruct',
  },
  {
    id: 'openai',
    nameKey: 'openai',
    baseUrl: 'https://api.openai.com/v1',
    modelPlaceholder: 'gpt-4o',
  },
  {
    id: 'mimo',
    nameKey: 'mimo',
    baseUrl: 'https://api.xiaomimimo.com/v1',
    modelPlaceholder: 'xiaomi/mimo-v2-flash',
  },
  {
    id: 'cometapi',
    nameKey: 'cometapi',
    baseUrl: 'https://api.cometapi.com/v1',
    modelPlaceholder: 'gpt-4o',
  },
  {
    id: 'openrouterFree',
    nameKey: 'openrouterFree',
    baseUrl: 'https://openrouter.ai/api/v1',
    modelPlaceholder: 'qwen/qwen3-coder:free',
  },
  {
    id: 'alibabaCoding',
    nameKey: 'alibabaCoding',
    baseUrl: 'https://coding-intl.dashscope.aliyuncs.com/v1',
    modelPlaceholder: 'qwen3-coder-plus',
  },
  {
    id: 'codingPlanX',
    nameKey: 'codingPlanX',
    baseUrl: 'https://api.codingplanx.ai/v1',
    modelPlaceholder: 'gpt-5-mini',
  },
  {
    id: 'custom',
    nameKey: 'custom',
    baseUrl: '',
    modelPlaceholder: '',
  },
] as const;

type LlmPresetId = typeof LLM_PRESETS[number]['id'];

const ASR_DEFAULT_RESOURCE_ID = 'volc.seedasr.sauc.duration';

// `volcengine` 走自建流式客户端；其余走 OpenAI 兼容 `/audio/transcriptions`
// （`coordinator.rs::is_whisper_compatible_provider`）。新增兼容厂商：
//   1. 在这里加一项 `{ id, nameKey, baseUrl, model }`；
//   2. `coordinator.rs::is_whisper_compatible_provider` 加同名 id；
//   3. 在 i18n 的 `settings.providers.presets.<nameKey>` 加文案。
const ASR_PRESETS = [
  { id: 'volcengine',   nameKey: 'asrVolcengine',   baseUrl: '',                                              model: ''                              },
  { id: 'siliconflow',  nameKey: 'asrSiliconflow',  baseUrl: 'https://api.siliconflow.cn/v1',                  model: 'FunAudioLLM/SenseVoiceSmall' },
  { id: 'zhipu',        nameKey: 'asrZhipu',        baseUrl: 'https://open.bigmodel.cn/api/paas/v4',           model: 'glm-asr-2512'                },
  { id: 'groq',         nameKey: 'asrGroq',         baseUrl: 'https://api.groq.com/openai/v1',                 model: 'whisper-large-v3-turbo'      },
  { id: 'whisper',      nameKey: 'asrWhisper',      baseUrl: 'https://api.openai.com/v1',                      model: 'whisper-1'                   },
  { id: 'foundry-local-whisper', nameKey: 'asrFoundryLocalWhisper', baseUrl: '',                              model: ''                              },
  // 本地 Qwen3-ASR：无 baseUrl/model 配置，模型在「模型设置」页下载与切换。
  { id: 'local-qwen3',  nameKey: 'asrLocalQwen3',   baseUrl: '',                                              model: ''                              },
] as const;

type AsrPresetId = typeof ASR_PRESETS[number]['id'];

function ProvidersSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  // `*Provider` 立即跟随 <select> 改动（受控组件必须实时反映用户输入）；
  // `committed*Provider` 才决定 CredentialField 的 key，仅在后端 active
  // 切换 + 默认值写完后再 commit。两者拆开是为了同时满足：
  //   - <select> 立刻显示用户的选择（issue #220 P2：codex 指出受控选不应等 await）
  //   - CredentialField 不要在后端 active 切完前 remount（issue #219：避免读到旧 entry）
  // `*SwitchSeq` 是 stale-write 守卫：用户 100ms 内连点两次时，先发的请求晚到不
  // 会覆盖后发的 commit。
  const [llmProvider, setLlmProvider] = useState<LlmPresetId>('ark');
  const [asrProvider, setAsrProvider] = useState<AsrPresetId>('volcengine');
  const [committedLlmProvider, setCommittedLlmProvider] = useState<LlmPresetId>('ark');
  const [committedAsrProvider, setCommittedAsrProvider] = useState<AsrPresetId>('volcengine');
  const llmSwitchSeqRef = useRef(0);
  const asrSwitchSeqRef = useRef(0);
  const [llmModelRevision, setLlmModelRevision] = useState(0);
  const [asrModelRevision, setAsrModelRevision] = useState(0);
  const os = detectOS();
  const visibleAsrPresets = ASR_PRESETS.filter(
    p => p.id !== 'foundry-local-whisper' || os === 'win',
  );

  useEffect(() => {
    if (!prefs) return;
    const knownLlm = LLM_PRESETS.find(x => x.id === prefs.activeLlmProvider);
    const llmId = knownLlm ? knownLlm.id : 'custom';
    setLlmProvider(llmId);
    setCommittedLlmProvider(llmId);
    const knownAsr = visibleAsrPresets.find(x => x.id === prefs.activeAsrProvider);
    const asrId = knownAsr ? knownAsr.id : 'volcengine';
    setAsrProvider(asrId);
    setCommittedAsrProvider(asrId);
  }, [prefs, os]);

  // issue #219 / #220 P2：
  //   1. 立刻 setLlmProvider —— 受控 <select> 必须反映用户最新选择。
  //   2. 用 seq 守卫每个 await：用户连点两次时旧请求晚到也不会盖掉新选择。
  //   3. 仅 setCommittedLlmProvider 之后 CredentialField 才 remount 读新 entry，
  //      此时后端 root.active.llm 已经是 id，lookup_account 落到正确 entry。
  //   4. endpoint/model 默认值仅在该 provider entry 该字段为空时才填，不覆盖用户自定义。
  const onLlmProviderChange = async (id: LlmPresetId) => {
    setLlmProvider(id);
    const seq = ++llmSwitchSeqRef.current;
    await setActiveLlmProvider(id);
    if (seq !== llmSwitchSeqRef.current) return;
    if (prefs) {
      const next = { ...prefs, activeLlmProvider: id };
      await updatePrefs(next);
      if (seq !== llmSwitchSeqRef.current) return;
    }
    const preset = LLM_PRESETS.find(p => p.id === id);
    if (preset?.baseUrl) {
      const existing = await readCredential('ark.endpoint');
      if (seq !== llmSwitchSeqRef.current) return;
      if (!existing) {
        await setCredential('ark.endpoint', preset.baseUrl);
        if (seq !== llmSwitchSeqRef.current) return;
      }
    }
    if (preset?.modelPlaceholder) {
      const existing = await readCredential('ark.model_id');
      if (seq !== llmSwitchSeqRef.current) return;
      if (!existing) {
        await setCredential('ark.model_id', preset.modelPlaceholder);
        if (seq !== llmSwitchSeqRef.current) return;
      }
    }
    setCommittedLlmProvider(id);
  };

  const onAsrProviderChange = async (id: AsrPresetId) => {
    setAsrProvider(id);
    const seq = ++asrSwitchSeqRef.current;
    await setActiveAsrProvider(id);
    if (seq !== asrSwitchSeqRef.current) return;
    if (prefs) {
      const next = { ...prefs, activeAsrProvider: id };
      await updatePrefs(next);
      if (seq !== asrSwitchSeqRef.current) return;
    }
    // OpenAI 兼容厂商首次切换时预填 baseUrl / model 默认值，省得用户必踩
    // 「跨厂商 model 名根本不一样」的坑；但用户已自定义后就不再覆盖。
    // volcengine 走另一套凭据，跳过。
    const preset = ASR_PRESETS.find(p => p.id === id);
    if (preset && preset.baseUrl) {
      const existing = await readCredential('asr.endpoint');
      if (seq !== asrSwitchSeqRef.current) return;
      if (!existing) {
        await setCredential('asr.endpoint', preset.baseUrl);
        if (seq !== asrSwitchSeqRef.current) return;
      }
    }
    if (preset && preset.model) {
      const existing = await readCredential('asr.model');
      if (seq !== asrSwitchSeqRef.current) return;
      if (!existing) {
        await setCredential('asr.model', preset.model);
        if (seq !== asrSwitchSeqRef.current) return;
      }
    }
    setCommittedAsrProvider(id);
  };

  // preset 决定 placeholder 与 default —— 必须跟着 committed*Provider 走，
  // 否则受控 <select> 立刻切到新厂商，但凭据字段还在显示旧 entry，placeholder
  // 会先于实际数据切换、视觉上对不上。
  const preset = LLM_PRESETS.find(p => p.id === committedLlmProvider) ?? LLM_PRESETS[LLM_PRESETS.length - 1];
  const asrPreset = visibleAsrPresets.find(p => p.id === committedAsrProvider);

  return (
    <>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginBottom: 10 }}>
        {t('settings.providers.credentialStorageNotice')}
      </div>
      <Card>
        <div style={{ marginBottom: 10 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>{t('settings.providers.llmTitle')}</div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>
            {t('settings.providers.llmDesc')}
          </div>
        </div>
        <SettingRow label={t('settings.providers.providerLabel')} desc={t('settings.providers.llmProviderDesc')}>
          <select
            value={llmProvider}
            onChange={e => onLlmProviderChange(e.target.value as LlmPresetId)}
            style={{ ...inputStyle, maxWidth: 200 }}
          >
            {LLM_PRESETS.map(p => (
              <option key={p.id} value={p.id}>{t(`settings.providers.presets.${p.nameKey}`)}</option>
            ))}
          </select>
        </SettingRow>
        <CredentialField key={`${committedLlmProvider}:api_key`} label={t('settings.providers.apiKeyLabel')} account="ark.api_key" mono mask />
        <CredentialField key={`${committedLlmProvider}:endpoint`} label={t('settings.providers.baseUrlLabel')} account="ark.endpoint"
          placeholder={preset.baseUrl || 'https://your-endpoint/v1'} />
        <CredentialField key={`${committedLlmProvider}:model:${llmModelRevision}`} label={t('settings.providers.modelLabel')} account="ark.model_id"
          placeholder={preset.modelPlaceholder || 'model-name'} mono />
        <ProviderTools key={committedLlmProvider} kind="llm" modelAccount="ark.model_id" onModelSelected={() => setLlmModelRevision(v => v + 1)} />
      </Card>

      <Card>
        <div style={{ marginBottom: 10 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>{t('settings.providers.asrTitle')}</div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>{t('settings.providers.asrDesc')}</div>
        </div>
        <SettingRow label={t('settings.providers.providerLabel')} desc={t('settings.providers.asrProviderDesc')}>
          <select
            value={asrProvider}
            onChange={e => onAsrProviderChange(e.target.value as AsrPresetId)}
            style={{ ...inputStyle, maxWidth: 200 }}
          >
            {visibleAsrPresets.map(p => (
              <option key={p.id} value={p.id}>{t(`settings.providers.presets.${p.nameKey}`)}</option>
            ))}
          </select>
        </SettingRow>
        {committedAsrProvider === 'volcengine' ? (
          <>
            <CredentialField
              key={`${committedAsrProvider}:app_key`}
              label={t('settings.providers.volcengineAppKeyLabel')}
              account="volcengine.app_key"
              mono
              mask
            />
            <CredentialField
              key={`${committedAsrProvider}:access_key`}
              label={t('settings.providers.volcengineAccessKeyLabel')}
              account="volcengine.access_key"
              mono
              mask
            />
            <CredentialField
              key={`${committedAsrProvider}:resource_id`}
              label={t('settings.providers.volcengineResourceIdLabel')}
              account="volcengine.resource_id"
              mono
              placeholder={ASR_DEFAULT_RESOURCE_ID} defaultValue={ASR_DEFAULT_RESOURCE_ID} />
            <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
              {t('settings.providers.volcengineMappingNote')}
            </div>
          </>
        ) : committedAsrProvider === 'local-qwen3' || committedAsrProvider === 'foundry-local-whisper' ? (
          <LocalAsrProviderHint provider={committedAsrProvider} selectedProvider={asrProvider} />
        ) : (
          <>
            <CredentialField key={`${committedAsrProvider}:api_key`} label={t('settings.providers.apiKeyLabel')} account="asr.api_key" mono mask />
            <CredentialField key={`${committedAsrProvider}:endpoint`} label={t('settings.providers.baseUrlLabel')} account="asr.endpoint"
              placeholder={asrPreset?.baseUrl || 'https://api.openai.com/v1'}
              defaultValue={asrPreset?.baseUrl || undefined} />
            <CredentialField key={`${committedAsrProvider}:model:${asrModelRevision}`} label={t('settings.providers.modelLabel')} account="asr.model"
              placeholder={asrPreset?.model || 'whisper-1'} />
            <ProviderTools kind="asr" modelAccount="asr.model" onModelSelected={() => setAsrModelRevision(v => v + 1)} />
          </>
        )}
      </Card>
    </>
  );
}

type ProviderToolStatus = 'idle' | 'loading' | 'success' | 'empty' | 'error';

function ProviderTools({ kind, modelAccount, onModelSelected }: { kind: 'llm' | 'asr'; modelAccount: string; onModelSelected: () => void }) {
  const { t } = useTranslation();
  const [models, setModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState('');
  const [status, setStatus] = useState<ProviderToolStatus>('idle');
  const [message, setMessage] = useState('');

  const setResult = (next: ProviderToolStatus, nextMessage: string) => {
    setStatus(next);
    setMessage(nextMessage);
  };

  const validate = async () => {
    setModels([]);
    setSelectedModel('');
    setResult('loading', t('settings.providers.validating'));
    try {
      const result = await validateProviderCredentials(kind);
      setResult(result.ok ? 'success' : 'error', t('settings.providers.validateSuccess'));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if ((kind === 'llm' && message === 'llmModelMissing') || (kind === 'asr' && message === 'asrModelMissing')) {
        setResult('empty', t('settings.providers.modelMissing'));
        return;
      }
      if (message === 'modelsEmpty') {
        setResult('empty', t('settings.providers.modelsEmpty'));
        return;
      }
      setResult('error', providerErrorMessage(error, t));
    }
  };

  const loadModels = async () => {
    setResult('loading', t('settings.providers.loadingModels'));
    try {
      const result = await listProviderModels(kind);
      setModels(result.models);
      if (result.models.length === 0) {
        setResult('empty', t('settings.providers.modelsEmpty'));
      } else {
        setSelectedModel('');
        setResult('success', t('settings.providers.modelsLoaded', { count: result.models.length }));
      }
    } catch (error) {
      setModels([]);
      setResult('error', providerErrorMessage(error, t));
    }
  };

  const applyModel = async (model: string) => {
    setResult('loading', t('common.saving'));
    try {
      await setCredential(modelAccount, model);
      setSelectedModel(model);
      onModelSelected();
      setResult('success', t('settings.providers.modelSaved', { model }));
    } catch (error) {
      setResult('error', providerErrorMessage(error, t));
    }
  };

  return (
    <SettingRow label={t('settings.providers.toolsLabel')} desc={t('settings.providers.toolsDesc')}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, width: '100%', maxWidth: 420 }}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
          <button onClick={validate} style={miniBtnStyle} disabled={status === 'loading'}>{t('settings.providers.validate')}</button>
          <button onClick={loadModels} style={miniBtnStyle} disabled={status === 'loading'}>{t('settings.providers.fetchModels')}</button>
          {models.length > 0 && (
            <select
              value={selectedModel}
              onChange={e => applyModel(e.target.value)}
              disabled={status === 'loading'}
              style={{ ...inputStyle, maxWidth: 220 }}
            >
              <option value="" disabled>{t('settings.providers.selectModel')}</option>
              {models.map(model => <option key={model} value={model}>{model}</option>)}
            </select>
          )}
        </div>
        {message && (
          <span style={{ fontSize: 11, color: status === 'error' ? 'var(--ol-warn)' : status === 'empty' ? 'var(--ol-ink-4)' : 'var(--ol-ok)', lineHeight: 1.4 }}>
            {message}
          </span>
        )}
      </div>
    </SettingRow>
  );
}


function providerErrorMessage(error: unknown, t: ReturnType<typeof useTranslation>['t']): string {
  const message = error instanceof Error ? error.message : String(error);
  if (message.startsWith('providerHttpStatus:')) {
    return t('settings.providers.providerHttpStatus', { status: message.split(':')[1] || '?' });
  }
  if (message === 'endpointMustUseHttps') return t('settings.providers.endpointMustUseHttps');
  if (message === 'endpointInvalid') return t('settings.providers.endpointInvalid');
  if (message === 'providerResponseTooLarge') return t('settings.providers.responseTooLarge');
  if (message === 'asrInvalidJson') return t('settings.providers.asrInvalidJson');
  if (message === 'asrMissingTextField') return t('settings.providers.asrMissingTextField');
  if (message === 'providerNetworkError') return t('common.networkError');
  if (message === 'providerReadResponseFailed' || message === 'providerClientInitFailed') return t('common.operationFailed');
  if (message === 'providerRequestTimeout') return t('settings.providers.requestTimeout');
  if (message.includes('API Key')) return t('settings.providers.apiKeyMissing');
  if (message.includes('Endpoint')) return t('settings.providers.endpointMissing');
  if (message.includes('timeout') || message.includes('超时')) return t('settings.providers.requestTimeout');
  return t('common.operationFailed');
}

type CredentialFieldStatus = 'idle' | 'saving' | 'saved' | 'readError' | 'saveError' | 'copied' | 'copyError';

interface CredentialFieldProps {
  label: string;
  account: string;
  placeholder?: string;
  mono?: boolean;
  mask?: boolean;
  defaultValue?: string;
}

function CredentialField({ label, account, placeholder, mono, mask, defaultValue }: CredentialFieldProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState('');
  const [revealed, setRevealed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState<CredentialFieldStatus>('idle');
  const debounceRef = useRef<number | null>(null);
  const statusRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    setDirty(false);
    setStatus('idle');
    setValue('');
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    readCredential(account)
      .then(v => {
        if (cancelled) return;
        setValue(v ?? '');
        setLoaded(true);
      })
      .catch(error => {
        if (cancelled) return;
        console.error('[settings] failed to read credential', account, error);
        setLoaded(true);
        setStatus('readError');
      });
    return () => {
      cancelled = true;
    };
  }, [account]);

  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      if (statusRef.current) clearTimeout(statusRef.current);
    };
  }, []);

  const showTemporaryStatus = (next: CredentialFieldStatus) => {
    setStatus(next);
    if (statusRef.current) clearTimeout(statusRef.current);
    statusRef.current = window.setTimeout(() => setStatus('idle'), 1600);
  };

  const save = async (v: string, force = false) => {
    if (!loaded || (!dirty && !force)) return;
    setStatus('saving');
    try {
      await setCredential(account, v);
      setDirty(false);
      showTemporaryStatus('saved');
    } catch (error) {
      console.error('[settings] failed to save credential', account, error);
      showTemporaryStatus('saveError');
    }
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const v = e.target.value;
    setValue(v);
    if (!loaded) return;
    setDirty(true);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = window.setTimeout(() => save(v, true), 300);
  };

  const onBlur = () => {
    if (!loaded || !dirty) return;
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    save(value, true);
  };

  const fillDefault = async () => {
    if (!loaded || !defaultValue) return;
    setValue(defaultValue);
    setDirty(true);
    await save(defaultValue, true);
  };

  const onCopy = async () => {
    if (!value || !loaded) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('Clipboard API unavailable');
      }
      await navigator.clipboard.writeText(value);
      showTemporaryStatus('copied');
    } catch (error) {
      console.error('[settings] failed to copy credential', account, error);
      showTemporaryStatus('copyError');
    }
  };

  const inputType = mask && !revealed ? 'password' : 'text';
  const disabled = !loaded;

  return (
    <SettingRow label={label}>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', maxWidth: 420 }}>
        <input
          type={inputType}
          value={value}
          placeholder={loaded ? placeholder : t('common.loading')}
          onChange={handleChange}
          onBlur={onBlur}
          disabled={disabled}
          style={{ ...inputStyle, fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
        />
        {defaultValue && !value && loaded && (
          <button onClick={fillDefault} title={t('settings.providers.fillDefault')} style={iconBtnStyle} disabled={!loaded}>
            <Icon name="check" size={13} />
          </button>
        )}
        {mask && (
          <button
            onClick={() => setRevealed(r => !r)}
            title={revealed ? t('common.hide') : t('common.show')}
            style={iconBtnStyle}
            disabled={disabled}
          >
            <Icon name="eye" size={14} />
          </button>
        )}
        <button
          onClick={onCopy}
          title={t('common.copy')}
          style={iconBtnStyle}
          disabled={!value || disabled}
        >
          <Icon name="copy" size={14} />
        </button>
        {status !== 'idle' && (
          <span
            style={{
              fontSize: 11,
              color: status.endsWith('Error') ? 'var(--ol-warn)' : 'var(--ol-ok)',
              whiteSpace: 'nowrap',
            }}
          >
            {status === 'saving'
              ? t('common.saving')
              : status === 'saved'
                ? t('common.saved')
                : status === 'copied'
                  ? t('common.copied')
                  : status === 'readError'
                    ? t('settings.providers.readFailed')
                    : t('common.operationFailed')}
          </span>
        )}
      </div>
    </SettingRow>
  );
}

const inputStyle: CSSProperties = {
  flex: 1, height: 32, padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, fontSize: 12.5,
  fontFamily: 'inherit', outline: 'none',
  background: 'var(--ol-surface-2)',
  width: '100%', maxWidth: 360,
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
};
const miniBtnStyle: CSSProperties = {
  height: 32, padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  color: 'var(--ol-ink-2)', cursor: 'default', flexShrink: 0,
  fontSize: 12, fontWeight: 500,
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
};

const recordingHotkeyControlWidth = 178;

const hotkeyRecorderButtonStyle: CSSProperties = {
  width: recordingHotkeyControlWidth,
  height: 32,
  padding: '0 8px 0 11px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8,
  background: 'var(--ol-surface-2)',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 8,
  fontFamily: 'var(--ol-font-mono)',
  fontSize: 12.5,
  cursor: 'default',
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
};

const recordingHotkeySegmentedStyle: CSSProperties = {
  width: recordingHotkeyControlWidth,
  display: 'inline-flex',
  padding: 2,
  borderRadius: 8,
  background: 'rgba(0,0,0,0.05)',
};

const recordingHotkeyGroupStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'auto',
  rowGap: 10,
  justifyItems: 'start',
};

const recordingHotkeyLineStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '64px auto',
  alignItems: 'center',
  columnGap: 10,
};

const recordingHotkeyFieldLabelStyle: CSSProperties = {
  fontSize: 12,
  color: 'var(--ol-ink-4)',
  textAlign: 'right',
  whiteSpace: 'nowrap',
};

const recordingHotkeyStatusStyle: CSSProperties = {
  marginLeft: 74,
  fontSize: 12,
  lineHeight: 1.3,
};

const hotkeyRecorderLabelStyle: CSSProperties = {
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
};

const hotkeyClearButtonStyle: CSSProperties = {
  width: 18,
  height: 18,
  borderRadius: 999,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  flexShrink: 0,
  background: 'rgba(0,0,0,0.2)',
  color: '#fff',
};

const iconBtnStyle: CSSProperties = {
  width: 32, height: 32,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  color: 'var(--ol-ink-3)', cursor: 'default', flexShrink: 0,
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
};

function ShortcutsSection() {
  const { t } = useTranslation();
  const { prefs, hotkey, capability, updatePrefs: savePrefs } = useHotkeySettings();

  if (!prefs || !hotkey || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const desc = capability.requiresAccessibilityPermission
    ? t('settings.shortcuts.descAcc')
    : t('settings.shortcuts.descNoAcc');
  const readonlyRows: Array<[string, string]> = [
    [t('settings.shortcuts.cancel'), 'Esc'],
    [t('settings.shortcuts.confirm'), t('settings.shortcuts.confirmHint')],
  ];
  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.shortcuts.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>{desc}</div>
      <SettingRow label={t('settings.shortcuts.startStop')}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, width: '100%' }}>
          <ShortcutRecorder
            value={prefs.dictationHotkey}
            alignRecordButton
            onSave={async binding => {
              await setDictationHotkey(binding);
              await savePrefs({ ...prefs, dictationHotkey: binding });
            }}
          />
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
            {hotkey.mode === 'hold' ? t('hotkey.modeHoldSuffix') : t('hotkey.modeToggleSuffix')}
          </div>
        </div>
      </SettingRow>
      <SettingRow label={t('translation.hotkey.title', 'Translation shortcut')}>
        <ShortcutRecorder
          value={prefs.translationHotkey}
          alignRecordButton
          onSave={async binding => {
            await setTranslationHotkey(binding);
            await savePrefs({ ...prefs, translationHotkey: binding });
          }}
        />
      </SettingRow>
      <SettingRow label={t('selectionAsk.hotkey.title')}>
        {prefs.qaHotkey ? (
          <ShortcutRecorder
            value={prefs.qaHotkey}
            alignRecordButton
            onSave={async binding => {
              await setQaHotkey(binding);
              await savePrefs({ ...prefs, qaHotkey: binding });
            }}
          />
        ) : (
          <button
            onClick={async () => {
              const binding = defaultQaShortcut();
              await setQaHotkey(binding);
              await savePrefs({ ...prefs, qaHotkey: binding });
            }}
            style={{ fontSize: 12, padding: '5px 14px', background: 'var(--ol-blue)', color: '#fff', border: 0, borderRadius: 6, fontFamily: 'inherit', fontWeight: 500, cursor: 'default' }}
          >
            {t('selectionAsk.hotkey.enable', 'Enable')}
          </button>
        )}
      </SettingRow>
      <SettingRow label={t('settings.shortcuts.switchStyle')}>
        <ShortcutRecorder
          value={prefs.switchStyleHotkey}
          alignRecordButton
          onSave={async binding => {
            await setSwitchStyleHotkey(binding);
            await savePrefs({ ...prefs, switchStyleHotkey: binding });
          }}
        />
      </SettingRow>
      <SettingRow label={t('settings.shortcuts.openApp')}>
        <ShortcutRecorder
          value={prefs.openAppHotkey}
          alignRecordButton
          onSave={async binding => {
            await setOpenAppHotkey(binding);
            await savePrefs({ ...prefs, openAppHotkey: binding });
          }}
        />
      </SettingRow>
      {readonlyRows.map(([k, v]) => (
        <SettingRow key={k} label={k}>
          <kbd style={{
            display: 'inline-flex', alignItems: 'center', gap: 4,
            padding: '4px 10px', fontSize: 12, fontFamily: 'var(--ol-font-mono)',
            borderRadius: 6, background: 'var(--ol-surface-2)',
            border: '0.5px solid var(--ol-line-strong)',
            boxShadow: '0 1px 0 rgba(0,0,0,0.04)',
            color: 'var(--ol-ink-2)',
          }}>{v}</kbd>
        </SettingRow>
      ))}
    </Card>
  );
}

function PermissionsSection() {
  const { t } = useTranslation();
  const [accessibility, setAccessibility] = useState<PermissionStatus | 'loading'>('loading');
  const [microphone, setMicrophone] = useState<PermissionStatus | 'loading'>('loading');
  const [hotkey, setHotkey] = useState<HotkeyStatus | null>(null);
  const [windowsIme, setWindowsIme] = useState<WindowsImeStatus | null>(null);
  const { capability } = useHotkeySettings();

  const refreshPermissions = async () => {
    const [a, m] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    setAccessibility(a);
    setMicrophone(m);
  };

  const refreshHotkey = async () => {
    setHotkey(await getHotkeyStatus());
  };

  const refreshWindowsIme = async () => {
    setWindowsIme(await getWindowsImeStatus());
  };

  useEffect(() => {
    refreshPermissions();
    refreshHotkey();
    refreshWindowsIme();
    const hotkeyId = window.setInterval(refreshHotkey, 1000);
    // 麦克风检查会短暂打开输入流，避免每秒探测导致隐私指示器频繁闪烁。
    const permissionId = window.setInterval(refreshPermissions, 10000);
    const onFocus = () => {
      refreshPermissions();
      refreshHotkey();
      refreshWindowsIme();
    };
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(hotkeyId);
      window.clearInterval(permissionId);
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  const reRequestAccessibility = async () => {
    await requestAccessibilityPermission();
    refreshPermissions();
  };

  const reRequestMicrophone = async () => {
    if (microphone === 'denied' || microphone === 'restricted') {
      await openSystemSettings('microphone');
      refreshPermissions();
      return;
    }
    const status = await requestMicrophonePermission();
    setMicrophone(status);
    if (status === 'denied' || status === 'restricted') {
      await openSystemSettings('microphone');
    }
    refreshPermissions();
  };

  const desc = capability?.requiresAccessibilityPermission
    ? t('settings.permissions.descAcc')
    : t('settings.permissions.descNoAcc');

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.permissions.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>
        {desc}
      </div>
      <SettingRow label={t('settings.permissions.micLabel')} desc={t('settings.permissions.micDesc')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <PermissionPill status={microphone} />
          {microphone !== 'granted' && microphone !== 'notApplicable' && microphone !== 'loading' && (
            <Btn variant="ghost" size="sm" onClick={reRequestMicrophone}>
              {microphone === 'denied' || microphone === 'restricted' ? t('settings.permissions.openSystem') : t('settings.permissions.grant')}
            </Btn>
          )}
        </div>
      </SettingRow>
      {capability?.requiresAccessibilityPermission && (
        <SettingRow label={t('settings.permissions.accLabel')} desc={t('settings.permissions.accDesc')}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <PermissionPill status={accessibility} />
            {accessibility !== 'granted' && accessibility !== 'notApplicable' && (
              <Btn variant="ghost" size="sm" onClick={reRequestAccessibility}>
                {t('settings.permissions.grant')}
              </Btn>
            )}
          </div>
        </SettingRow>
      )}
      <SettingRow
        label={t('settings.permissions.hotkeyLabel')}
        desc={capability ? t('settings.permissions.hotkeyDescWithAdapter', { adapter: adapterDisplayName(capability.adapter) }) : t('settings.permissions.hotkeyDescPlain')}
      >
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', minWidth: 0 }}>
          <HotkeyStatusPill status={hotkey} />
          {hotkey?.message && (
            <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {hotkey.message}
            </span>
          )}
        </div>
      </SettingRow>
      {windowsIme?.state !== 'notWindows' && (
        <SettingRow
          label={t('settings.permissions.windowsImeLabel')}
          desc={t('settings.permissions.windowsImeDesc')}
        >
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', minWidth: 0 }}>
            <WindowsImeStatusPill status={windowsIme} />
            {windowsIme && (
              <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {t(`settings.permissions.windowsIme.${windowsIme.state}`)}
              </span>
            )}
          </div>
        </SettingRow>
      )}
      <SettingRow label={t('settings.permissions.networkLabel')} desc={t('settings.permissions.networkDesc')}>
        <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.networkOk')}</Pill>
      </SettingRow>
    </Card>
  );
}

function PermissionPill({ status }: { status: PermissionStatus | 'loading' }) {
  const { t } = useTranslation();
  if (status === 'loading') {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status === 'granted') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.granted')}</Pill>;
  }
  if (status === 'notApplicable') {
    return <Pill tone="default">{t('settings.permissions.notApplicable')}</Pill>;
  }
  if (status === 'denied' || status === 'restricted') {
    return <Pill tone="outline">{t('settings.permissions.denied')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.indeterminate')}</Pill>;
}

function LanguageSection() {
  const { t } = useTranslation();
  const { updatePrefs } = useHotkeySettings();
  const [pref, setPref] = useState<SupportedLocale | typeof FOLLOW_SYSTEM>(getLocalePreference());

  const apply = async (next: SupportedLocale | typeof FOLLOW_SYSTEM) => {
    setPref(next);
    const resolved = await setLocalePreference(next);
    const localePrefs = outputPrefsForLocale(resolved);
    await updatePrefs(current => {
      if (
        current.chineseScriptPreference === localePrefs.chineseScriptPreference &&
        current.outputLanguagePreference === localePrefs.outputLanguagePreference
      ) {
        return current;
      }
      return { ...current, ...localePrefs };
    });
  };

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.language.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>{t('settings.language.desc')}</div>
      <SettingRow label={t('settings.language.label')} desc={t('settings.language.labelDesc')}>
        <select
          value={pref}
          onChange={e => apply(e.target.value as SupportedLocale | typeof FOLLOW_SYSTEM)}
          style={{ ...inputStyle, maxWidth: 220 }}
        >
          <option value={FOLLOW_SYSTEM}>{t('settings.language.followSystem')}</option>
          <option value="zh-CN">{t('settings.language.zh')}</option>
          <option value="zh-TW">{t('settings.language.zhTW')}</option>
          <option value="en">{t('settings.language.en')}</option>
          <option value="ja">{t('settings.language.ja')}</option>
          <option value="ko">{t('settings.language.ko')}</option>
        </select>
      </SettingRow>
      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 8, lineHeight: 1.6 }}>
        {t('settings.language.restartHint')}
      </div>
    </Card>
  );
}

// AboutSection 已移除：内容并入 SettingsModal 的 AboutMini，避免设置内外两个"关于"重复入口。

export function AboutUpdateControl({ tagline }: { tagline: string }) {
  const { t } = useTranslation();
  const u = useAutoUpdate();
  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 2 }}>
        <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>{tagline} 路 {APP_VERSION_LABEL}</span>
        <Btn variant="ghost" size="sm" onClick={u.checkForUpdates} disabled={u.checking || u.busy}>
          {u.checking ? t('settings.about.checkingUpdate') : t('settings.about.checkUpdateBtn')}
        </Btn>
      </div>
      {(u.status === 'none' || u.status === 'error') && (
        <div style={{ fontSize: 11, color: u.status === 'error' ? 'var(--ol-err)' : 'var(--ol-ink-4)', marginTop: 4 }}>
          {u.status === 'none' ? t('settings.about.upToDate') : t('settings.about.updateError')}
        </div>
      )}
      {isDialogStatus(u.status) && (
        <UpdateDialog
          status={u.status}
          version={u.version}
          progress={u.progress}
          downloaded={u.downloaded}
          contentLength={u.contentLength}
          onInstall={u.installUpdate}
          onClose={u.dismissDialog}
        />
      )}
    </>
  );
}

function HotkeyStatusPill({ status }: { status: HotkeyStatus | null }) {
  const { t } = useTranslation();
  if (!status) {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status.state === 'installed') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.hotkeyInstalled')}</Pill>;
  }
  if (status.state === 'starting') {
    return <Pill tone="default">{t('settings.permissions.hotkeyStarting')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.hotkeyFailed')}</Pill>;
}

function WindowsImeStatusPill({ status }: { status: WindowsImeStatus | null }) {
  const { t } = useTranslation();
  if (!status) {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status.state === 'installed') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.windowsImeInstalled')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.windowsImeUnavailable')}</Pill>;
}

function adapterDisplayName(adapter: HotkeyCapability['adapter'] | HotkeyStatus['adapter']) {
  if (adapter === 'macEventTap') return i18n.t('hotkey.adapter.macEventTap');
  if (adapter === 'windowsLowLevel') return i18n.t('hotkey.adapter.windowsLowLevel');
  return i18n.t('hotkey.adapter.rdev');
}

/// 本地 Qwen3-ASR 在 Settings → 服务商区里**不**让用户填空——展示当前激活模型
/// 是否已下载、列出所有已下载模型 + 删除按钮，并提示性能/质量预期，引导跳到
/// 「模型设置」页做下载。
function LocalAsrProviderHint({
  provider,
  selectedProvider,
}: {
  provider: 'local-qwen3' | 'foundry-local-whisper';
  selectedProvider: AsrPresetId;
}) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<LocalAsrSettings | null>(null);
  const [models, setModels] = useState<LocalAsrModelStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const refreshSeqRef = useRef(0);
  const providerStateRef = useRef({ provider, selectedProvider });
  providerStateRef.current = { provider, selectedProvider };

  const qwenReadyForFetch = () => {
    const state = providerStateRef.current;
    return state.provider === 'local-qwen3' && state.selectedProvider === 'local-qwen3';
  };

  const refresh = async (seq: number) => {
    try {
      const [s, list] = await Promise.all([getLocalAsrSettings(), listLocalAsrModels()]);
      if (seq !== refreshSeqRef.current) {
        return;
      }
      setSettings(s);
      setModels(list);
    } catch (err) {
      if (seq !== refreshSeqRef.current) {
        return;
      }
      console.warn('[settings] load local asr status failed', err);
    } finally {
      if (seq === refreshSeqRef.current) {
        setLoading(false);
      }
    }
  };

  const beginRefresh = () => {
    const seq = ++refreshSeqRef.current;
    setSettings(null);
    setModels([]);
    setDeletingId(null);
    if (provider !== selectedProvider) {
      setLoading(true);
      return;
    }
    if (provider === 'foundry-local-whisper') {
      setLoading(false);
      return;
    }
    setLoading(true);
    void refresh(seq);
  };

  useEffect(() => {
    beginRefresh();
    return () => {
      refreshSeqRef.current += 1;
    };
  }, [provider, selectedProvider]);

  const goToLocalAsr = () => {
    window.dispatchEvent(new CustomEvent(NAVIGATE_LOCAL_ASR_EVENT));
  };

  const handleDelete = async (modelId: string) => {
    const seq = refreshSeqRef.current;
    if (!qwenReadyForFetch()) {
      return;
    }
    setDeletingId(modelId);
    try {
      await deleteLocalAsrModel(modelId);
      if (seq !== refreshSeqRef.current || !qwenReadyForFetch()) {
        return;
      }
      beginRefresh();
    } catch (err) {
      console.warn('[settings] delete local model failed', err);
    } finally {
      if (seq === refreshSeqRef.current && provider === 'local-qwen3') {
        setDeletingId(null);
      }
    }
  };

  const hintKey = provider === 'foundry-local-whisper'
    ? 'settings.providers.foundryLocalAsrHint'
    : 'settings.providers.localAsrHint';

  if (loading) {
    return (
      <div style={{ padding: '12px 0', fontSize: 12.5, color: 'var(--ol-ink-4)' }}>
        {t('common.loading')}
      </div>
    );
  }

  const active = models.find(m => m.id === settings?.activeModel);
  const isReady = active?.isDownloaded ?? false;
  const downloaded = models.filter(m => m.isDownloaded);

  if (provider === 'foundry-local-whisper') {
    return (
      <div style={{ padding: '8px 0 4px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
          {t(hintKey)}
        </div>
        <div>
          <Btn variant="ghost" size="sm" onClick={goToLocalAsr}>
            {t('settings.providers.localAsrManage')}
          </Btn>
        </div>
      </div>
    );
  }

  return (
    <div style={{ padding: '8px 0 4px', display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* 性能/质量预期警告 —— 用户硬要求要写清楚 */}
      <div
        style={{
          padding: '10px 12px',
          background: 'rgba(255, 215, 130, 0.18)',
          borderRadius: 8,
          fontSize: 12.5,
          color: 'var(--ol-ink-2)',
          lineHeight: 1.6,
        }}>
        ⚠️ {t('settings.providers.localAsrPerformanceWarning')}
      </div>

      <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
        {t(hintKey)}
      </div>

      {/* 当前激活模型状态 + 跳转按钮 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <Pill tone={isReady ? 'ok' : 'outline'} size="sm">
          {isReady
            ? t('settings.providers.localAsrReady', { model: active?.id ?? '' })
            : t('settings.providers.localAsrNotReady', { model: settings?.activeModel ?? '' })}
        </Pill>
        <Btn variant={isReady ? 'ghost' : 'primary'} size="sm" onClick={goToLocalAsr}>
          {isReady
            ? t('settings.providers.localAsrManage')
            : t('settings.providers.localAsrGoDownload')}
        </Btn>
      </div>

      {/* 已下载模型列表 + 删除按钮（用户：已下载的项目要在旁边显示 + 提供删除） */}
      {downloaded.length > 0 && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <div style={{ fontSize: 11.5, fontWeight: 600, color: 'var(--ol-ink-4)', letterSpacing: '.04em', textTransform: 'uppercase' }}>
            {t('settings.providers.localAsrDownloadedTitle')}
          </div>
          {downloaded.map(m => (
            <div
              key={m.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                padding: '6px 10px',
                borderRadius: 6,
                background: 'rgba(0,0,0,0.03)',
                fontSize: 12.5,
                color: 'var(--ol-ink-2)',
              }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
                <span style={{ fontWeight: 500 }}>{m.id}</span>
                <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
                  {formatBytes(m.downloadedBytes)}
                </span>
              </div>
              <Btn
                variant="ghost"
                size="sm"
                disabled={deletingId === m.id}
                onClick={() => void handleDelete(m.id)}>
                {t('settings.providers.localAsrDelete')}
              </Btn>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(0)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
