// 通用 → 录音与输入：录音快捷键 / 方式 / 麦克风 / 胶囊 / 静音，
// 外加「插入与剪贴板」「启动」两个折叠组。流式输入也并到这里（属于插入行为）。
// 自 Settings.tsx 的 RecordingSection 拆出，录音相关逻辑零改动。

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { playRecordStartCue } from '../../lib/audioCue';
import { defaultDictationHotkey } from '../../lib/hotkey';
import { emitSaved } from '../../lib/savedEvent';
import { isHotkeyModeMigrationNoticeActive } from '../../lib/hotkeyMigration';
import {
  isTauri,
  listMicrophoneDevices,
  setDictationHotkey,
} from '../../lib/ipc';
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
} from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite } from '../../components/ui/SelectLite';
import { Card, Collapsible } from '../_atoms';
import { SectionTitle, SettingRow, Toggle, inputStyle, segmentedTrackStyle } from './shared';
import { MicrophoneSelect } from './MicrophoneSelect';
import { detectOS } from '../../components/WindowChrome';

/** 「静音后自动停止」的可选静音时长（秒），见 issue #860。 */
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

  // 兼容旧 Windows 配置：Shift+Insert 仍保留在跨平台类型/后端中供 Linux 使用，
  // 但 Windows 已不再提供该选项；进入设置时迁移为 Ctrl+V，避免下拉框无匹配值。
  useEffect(() => {
    if (os !== 'win' || prefs?.pasteShortcut !== 'shiftInsert') return;
    void savePrefs(current => {
      if (current.pasteShortcut !== 'shiftInsert') return current;
      return { ...current, pasteShortcut: 'ctrlV' };
    }).catch(error => {
      console.warn('[settings] migrate Windows paste shortcut failed', error);
    });
  }, [os, prefs?.pasteShortcut, savePrefs]);

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

  const saveKeyboardListAffectingPrefs = useCallback(async (nextPrefs: UserPreferences) => {
    try {
      await savePrefs(nextPrefs);
    } catch (error) {
      console.error('[settings] keyboard list visibility pref save failed', error);
      emitSaved('failed', t('settings.recording.windowsShowOpenlessInKeyboardListError'));
      await refresh();
    }
  }, [savePrefs, refresh, t]);

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
  const effectivePasteShortcut = os === 'win' && prefs.pasteShortcut === 'shiftInsert'
    ? 'ctrlV'
    : prefs.pasteShortcut;

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
  const onWindowsInsertionModeChange = (windowsInsertionMode: WindowsInsertionMode) =>
    void saveKeyboardListAffectingPrefs({
      ...prefs,
      windowsInsertionMode,
      windowsSendInputInsertionOnly: windowsInsertionMode === 'sendInput',
    });
  const onWindowsSendInputNewlineModeChange = (windowsSendInputNewlineMode: WindowsSendInputNewlineMode) =>
    savePrefs({ ...prefs, windowsSendInputNewlineMode });
  const onWindowsShowOpenlessInKeyboardListChange = (
    windowsShowOpenlessInKeyboardList: boolean,
  ) => void saveKeyboardListAffectingPrefs({ ...prefs, windowsShowOpenlessInKeyboardList });
  const onStartMinimizedChange = (startMinimized: boolean) =>
    savePrefs({ ...prefs, startMinimized });
  const onAutoUpdateCheckChange = (autoUpdateCheck: boolean) =>
    savePrefs({ ...prefs, autoUpdateCheck });

  // 录音方式（按住说话 / 自动等）横向选框的滑动指示块：跟随选中项移动，
  // left/width 过渡就是切换动画。按钮的 offsetParent 就是 track（position:relative），
  // offsetLeft 与绝对定位 thumb 的 containing block 同原点（track padding 边），
  // 直接赋值即可；useLayoutEffect 在 paint 前定位，首帧无闪动。
  // 依赖必须含 showDesktopHotkey：prefs 与 platformCaps 异步加载，首次进入时可能
  // 出现「activeMode 已定但按钮未挂载」的时序（先 prefs 后 caps），effect 提前
  // return 后按钮才挂载——不加这个依赖 thumb 就永远不定位（用户反馈：首次进入
  // 设置不显示当前选择的录音方式）。
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
    // 语言切换（按钮文案重排）/ 窗口缩放都会改变按钮宽高，ResizeObserver 兜底
    // 重测，避免 thumb 停在旧位置（pr_agent #912 反馈）。
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
            sideSpecificModifiers
            // 录音快捷键是核心热键，Rust 端不接受 null，不可停用——置灰并提示。
            disableDisabled
            disableHint={t('settings.recording.comboDisableHint')}
            onSave={async binding => {
              await setDictationHotkey(binding);
              await savePrefs({ ...prefs, dictationHotkey: binding });
            }}
            onReset={async () => {
              const binding = defaultDictationHotkey();
              await setDictationHotkey(binding);
              await savePrefs({ ...prefs, dictationHotkey: binding });
            }}
          />
        </SettingRow>
        )}
        {showDesktopHotkey && (
        <SettingRow label={t('settings.recording.modeLabel')} desc={t('settings.recording.modeDesc')}>
          <div ref={modeTrackRef} style={{ ...segmentedTrackStyle, position: 'relative' }}>
            {/* 滑动指示块：选中态背景/投影移到这上面，切换时 left/width 平滑过渡；
                按钮本身只变文字颜色。measure 前（首帧 layout effect 前）不渲染，
                paint 前已定位，无闪动。 */}
            {modeThumb && (
              <div
                style={{
                  position: 'absolute',
                  top: 2, bottom: 2,
                  left: modeThumb.left,
                  width: modeThumb.width,
                  borderRadius: 6,
                  background: 'var(--ol-segmented-active-bg)',
                  boxShadow: 'var(--ol-segmented-active-shadow)',
                  transition: 'left 0.18s var(--ol-motion-soft), width 0.18s var(--ol-motion-soft)',
                  pointerEvents: 'none',
                }}
              />
            )}
            {choices.map(([v, l]) => (
              <button
                key={v}
                ref={el => { if (el) modeButtonsRef.current.set(v, el); }}
                onClick={() => onModeChange(v)}
                style={{
                  padding: '5px 14px', fontSize: 12, fontWeight: 500,
                  border: 0, borderRadius: 6, fontFamily: 'inherit',
                  position: 'relative', zIndex: 1,
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
        // 「静音后自动停止」只在切换式（toggle）模式下可用。外层容器恒渲染，
        // 录音方式从按住/自动切到切换式时整组从下方拉出、切走时收回——与开关
        // 控制秒数行的动画同款（grid 0fr→1fr），不再「突然跳出」。收起时内容
        // 仍在 DOM 里（overflow hidden），动画结束高度归 0、不占布局；inert
        // 把折叠态控件移出 tab 顺序与 a11y 树（与 Collapsible 同款，pr_agent 反馈）。
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
              {/* 开关行只放 Toggle：秒数下拉移出本行，避免开关时行内内容变宽导致抽搐。 */}
              <Toggle
                on={prefs.silenceAutoStopEnabled}
                onToggle={next => savePrefs({ ...prefs, silenceAutoStopEnabled: next })}
              />
            </SettingRow>
            {/* 开启后在本行下方展开「静音时长」选择行。展开/收起走 grid 0fr→1fr
                过渡（嵌套在外层容器内，随外层一起拉出/收回）。终点高度固定
                （SettingRow 自身高度），反复开关不会跳动。 */}
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
                  {/* 与麦克风下拉同款视觉：SelectLite 默认触发器底色 + 固定 200 宽 →
                      右边缘与「MacBook Air 麦克风」对齐（control 区域同起点同宽）。 */}
                  <SelectLite
                    value={String(
                      silenceAutoStopOptions.includes(prefs.silenceAutoStopSeconds)
                        ? prefs.silenceAutoStopSeconds
                        : 3,
                    )}
                    onChange={next => savePrefs({ ...prefs, silenceAutoStopSeconds: Number(next) })}
                    options={silenceAutoStopOptions.map(value => ({
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
        <SettingRow label={t('settings.recording.microphoneLabel')} desc={t('settings.recording.microphoneDesc')}>
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
        <SettingRow label={t('settings.recording.capsuleLabel')} desc={t('settings.recording.capsuleDesc')}>
          <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
        </SettingRow>
        )}
        {os !== 'linux' && !isAndroid && (
        <SettingRow label={t('settings.recording.capsuleStyleLabel')}>
          <SelectLite
            value={prefs.capsuleStyle ?? 'siri'}
            onChange={next => savePrefs({ ...prefs, capsuleStyle: next as CapsuleStyle })}
            options={[
              { value: 'siri', label: t('settings.recording.capsuleStyleSiri') },
              { value: 'classic', label: t('settings.recording.capsuleStyleClassic') },
            ]}
            ariaLabel={t('settings.recording.capsuleStyleLabel')}
            // 与麦克风/语言等下拉同款视觉：走 SelectLite 默认触发器底色
            //（--ol-select-trigger-bg），而不是 inputStyle 的 surface-2；宽度
            // 220 与同区粘贴快捷键行、语言区保持一致（minWidth 200 防缩窄）。
            style={{ maxWidth: 220, minWidth: 200 }}
          />
        </SettingRow>
        )}
        <SettingRow label={t('settings.recording.muteDuringRecordingLabel')} desc={t('settings.recording.muteDuringRecordingDesc')}>
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
        <SettingRow label={t('settings.recording.restoreClipboardLabel')} desc={t('settings.recording.restoreClipboardDesc')}>
          <Toggle on={prefs.restoreClipboardAfterPaste} onToggle={onRestoreClipboardChange} />
        </SettingRow>
        {capability.adapter !== 'macEventTap' && (
          <SettingRow label={t('settings.recording.pasteShortcutLabel')} desc={t('settings.recording.pasteShortcutDesc')}>
            <SelectLite
              value={effectivePasteShortcut}
              onChange={next => onPasteShortcutChange(next as PasteShortcut)}
              options={[
                { value: 'ctrlV', label: t('settings.recording.pasteShortcutCtrlV') },
                { value: 'ctrlShiftV', label: t('settings.recording.pasteShortcutCtrlShiftV') },
                // 这个「粘贴与剪贴板」组只在 Windows 出现（showDesktopInsert 已排除 Linux，mac 走
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
              value={prefs.windowsInsertionMode ?? (prefs.windowsSendInputInsertionOnly ? 'sendInput' : 'tsf')}
              onChange={next => onWindowsInsertionModeChange(next as WindowsInsertionMode)}
              options={[
                { value: 'tsf', label: t('settings.recording.windowsInsertionModeTsf') },
                { value: 'sendInput', label: t('settings.recording.windowsInsertionModeSendInput') },
                { value: 'paste', label: t('settings.recording.windowsInsertionModePaste') },
              ]}
              ariaLabel={t('settings.recording.windowsInsertionModeLabel')}
              style={{ ...inputStyle, maxWidth: 260 }}
            />
          </SettingRow>
        )}
        {capability.adapter === 'windowsLowLevel'
          && (prefs.windowsInsertionMode === 'sendInput' || prefs.windowsSendInputInsertionOnly) && (
          <SettingRow
            label={t('settings.recording.windowsSendInputNewlineModeLabel')}
            desc={t('settings.recording.windowsSendInputNewlineModeDesc')}
          >
            <SelectLite
              value={prefs.windowsSendInputNewlineMode ?? 'enter'}
              onChange={next => onWindowsSendInputNewlineModeChange(next as WindowsSendInputNewlineMode)}
              options={[
                { value: 'enter', label: t('settings.recording.windowsSendInputNewlineModeEnter') },
                { value: 'shiftEnter', label: t('settings.recording.windowsSendInputNewlineModeShiftEnter') },
                { value: 'crlf', label: t('settings.recording.windowsSendInputNewlineModeCrLf') },
              ]}
              ariaLabel={t('settings.recording.windowsSendInputNewlineModeLabel')}
              style={{ ...inputStyle, maxWidth: 260 }}
            />
          </SettingRow>
        )}
        {capability.adapter === 'windowsLowLevel'
          && (prefs.windowsInsertionMode === 'sendInput' || prefs.windowsSendInputInsertionOnly) && (
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
