import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { ShortcutRecorder } from './ShortcutRecorder';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getCredentials,
  getHotkeyStatus,
  isTauri,
  openExternal,
  openSystemSettings,
  readCredential,
  requestAccessibilityPermission,
  requestMicrophonePermission,
  setActiveAsrProvider,
  setActiveStylePack,
  setCredential,
  setDictationHotkey,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
} from '../lib/ipc';
import { formatComboLabel } from '../lib/hotkey';
import type { CredentialsStatus, HotkeyStatus, PermissionStatus, ShortcutBinding } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';

export const ONBOARDING_COMPLETE_KEY = 'openless:onboarding-complete:v3';
export const REQUIRED_ONBOARDING_VERSION = 3;

const VOLCENGINE_SETUP_URL = 'https://github.com/appergb/openless/blob/main/docs/volcengine-setup.md';
const VOLCENGINE_LOGIN_URL = 'https://console.volcengine.com/auth/login/';
const VOLCENGINE_APP_CREATE_URL = 'https://console.volcengine.com/speech/app?opt=create';
const VOLCENGINE_SERVICE_URL = 'https://console.volcengine.com/speech/service/10038?AppID=&opt=create';
const BUILTIN_RAW_STYLE_PACK_ID = 'builtin.raw';
const VOLCENGINE_PROVIDER_ID = 'volcengine';
const VOLCENGINE_RESOURCE_ID = 'volc.seedasr.sauc.duration';
const LOCAL_ASR_PROVIDER_IDS = new Set(['local-qwen3', 'foundry-local-whisper']);

type SlideId = 'mic' | 'shortcut' | 'asr';
type MicTestState = 'idle' | 'listening' | 'heard' | 'error';
type HotkeyTestState = 'idle' | 'listening' | 'matched' | 'missed';
type SaveState = 'idle' | 'saving' | 'saved' | 'error';
type StatusTone = 'neutral' | 'success' | 'warning';
type MotionPhase = 'settled' | 'leaving' | 'entering';

const SLIDES: SlideId[] = ['mic', 'shortcut', 'asr'];
const DEFAULT_SHORTCUT: ShortcutBinding = { primary: 'RightControl', modifiers: [] };

export interface OnboardingCompleteOptions {
  openSettingsSection?: 'providers' | 'advanced';
}

interface OnboardingProps {
  onComplete: (options?: OnboardingCompleteOptions) => void;
}

export function Onboarding({ onComplete }: OnboardingProps) {
  const { t } = useTranslation();
  const { prefs, capability, loading: settingsLoading, updatePrefs } = useHotkeySettings();
  const [slideIndex, setSlideIndex] = useState(0);
  const [motionPhase, setMotionPhase] = useState<MotionPhase>('settled');
  const [accessibility, setAccessibility] = useState<PermissionStatus>('notDetermined');
  const [microphone, setMicrophone] = useState<PermissionStatus>('notDetermined');
  const [credentials, setCredentials] = useState<CredentialsStatus | null>(null);
  const [hotkeyStatus, setHotkeyStatus] = useState<HotkeyStatus | null>(null);
  const [permissionBusy, setPermissionBusy] = useState<'accessibility' | 'microphone' | null>(null);
  const [micLevel, setMicLevel] = useState(0);
  const [micMonitoring, setMicMonitoring] = useState(false);
  const [micTestState, setMicTestState] = useState<MicTestState>('idle');
  const [micTestError, setMicTestError] = useState<string | null>(null);
  const [hotkeyTestState, setHotkeyTestState] = useState<HotkeyTestState>('idle');
  const [shortcutEditorOpen, setShortcutEditorOpen] = useState(false);
  const [volcGuideOpen, setVolcGuideOpen] = useState(false);
  const [asrSaveState, setAsrSaveState] = useState<SaveState>('idle');
  const [asrForm, setAsrForm] = useState({ appKey: '', accessKey: '' });
  const [viewport, setViewport] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));
  const autoMicStartedRef = useRef(false);
  const micUnlistenRef = useRef<(() => void) | null>(null);
  const micMockTimerRef = useRef<number | null>(null);
  const hotkeyResetTimerRef = useRef<number | null>(null);

  const activeSlide = SLIDES[slideIndex];
  const isCompact = viewport.width < 640 || viewport.height < 560;
  const isShortViewport = viewport.height < 520;
  const microphoneReady = permissionReady(microphone);
  const accessibilityReady = permissionReady(accessibility);
  const shortcutBinding = prefs?.dictationHotkey ?? DEFAULT_SHORTCUT;
  const shortcutConfigured = Boolean(shortcutBinding.primary);
  const shortcutReady = shortcutConfigured && (!capability?.requiresAccessibilityPermission || accessibilityReady);
  const asrReady = Boolean(credentials?.volcengineConfigured || asrSaveState === 'saved');
  const asrHasInput = Boolean(asrForm.appKey.trim() || asrForm.accessKey.trim());
  const asrFormValid = Boolean(asrForm.appKey.trim() && asrForm.accessKey.trim());

  const copy = useCallback((key: string, fallback: string) => (
    t(key, { defaultValue: fallback }) as string
  ), [t]);

  const refreshStatus = useCallback(async () => {
    const [a, m, c, h] = await Promise.allSettled([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
      getCredentials(),
      getHotkeyStatus(),
    ]);
    if (a.status === 'fulfilled') setAccessibility(a.value);
    if (m.status === 'fulfilled') setMicrophone(m.value);
    if (c.status === 'fulfilled') setCredentials(c.value);
    if (h.status === 'fulfilled') setHotkeyStatus(h.value);
  }, []);

  useEffect(() => {
    void refreshStatus();
    const id = window.setInterval(() => void refreshStatus(), 2500);
    const onFocus = () => void refreshStatus();
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(id);
      window.removeEventListener('focus', onFocus);
    };
  }, [refreshStatus]);

  const ensureOnlineAsrDefault = useCallback(async () => {
    const credentialsProvider = credentials?.activeAsrProvider ?? '';
    const prefsProvider = prefs?.activeAsrProvider ?? '';
    const credentialsOnLocal = isLocalAsrProvider(credentialsProvider);
    const prefsOnLocal = isLocalAsrProvider(prefsProvider);
    if (!credentialsOnLocal && !prefsOnLocal) return;

    if (credentialsOnLocal) {
      await setActiveAsrProvider(VOLCENGINE_PROVIDER_ID);
    }
    if (prefsOnLocal) {
      await updatePrefs(current => ({
        ...current,
        activeAsrProvider: VOLCENGINE_PROVIDER_ID,
      }));
    }
    await refreshStatus();
  }, [credentials?.activeAsrProvider, prefs?.activeAsrProvider, refreshStatus, updatePrefs]);

  useEffect(() => {
    void ensureOnlineAsrDefault().catch(error => {
      console.warn('[onboarding] failed to switch local ASR back to online default', error);
    });
  }, [ensureOnlineAsrDefault]);

  useEffect(() => {
    let cancelled = false;
    async function loadVolcengineCredentials() {
      const [appKey, accessKey] = await Promise.all([
        readCredential('volcengine.app_key').catch(() => null),
        readCredential('volcengine.access_key').catch(() => null),
      ]);
      if (cancelled) return;
      setAsrForm({
        appKey: appKey ?? '',
        accessKey: accessKey ?? '',
      });
    }
    void loadVolcengineCredentials();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const onResize = () => setViewport({ width: window.innerWidth, height: window.innerHeight });
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const stopMicTest = useCallback(() => {
    micUnlistenRef.current?.();
    micUnlistenRef.current = null;
    if (micMockTimerRef.current !== null) {
      window.clearInterval(micMockTimerRef.current);
      micMockTimerRef.current = null;
    }
    setMicLevel(0);
    setMicMonitoring(false);
    setMicTestState(current => current === 'heard' ? current : 'idle');
    void stopMicrophoneLevelMonitor();
  }, []);

  useEffect(() => () => {
    micUnlistenRef.current?.();
    if (micMockTimerRef.current !== null) {
      window.clearInterval(micMockTimerRef.current);
    }
    void stopMicrophoneLevelMonitor();
  }, []);

  const startMicTest = useCallback(async () => {
    stopMicTest();
    setMicTestError(null);
    setMicLevel(0);
    setMicMonitoring(true);
    setMicTestState('listening');
    try {
      let status = microphone;
      if (!permissionReady(status)) {
        status = await requestMicrophonePermission();
        setMicrophone(status);
      }
      if (!permissionReady(status)) {
        setMicMonitoring(false);
        setMicTestState('error');
        setMicTestError(copy('onboarding.micPermissionBlocked', '请先允许 OpenLess 使用麦克风。'));
        if (status === 'denied' || status === 'restricted') {
          await openSystemSettings('microphone');
        }
        return;
      }

      if (isTauri) {
        const { listen } = await import('@tauri-apps/api/event');
        const unlisten = await listen<{ level: number }>('microphone:level', event => {
          const level = Math.max(0, Math.min(1, event.payload.level ?? 0));
          setMicLevel(level);
          if (level > 0.08) {
            setMicTestState('heard');
          }
        });
        micUnlistenRef.current = unlisten;
        await startMicrophoneLevelMonitor(prefs?.microphoneDeviceName ?? '');
      } else {
        micMockTimerRef.current = window.setInterval(() => {
          const level = 0.2 + Math.random() * 0.5;
          setMicLevel(level);
          setMicTestState('heard');
        }, 120);
      }
    } catch (error) {
      console.warn('[onboarding] microphone test failed', error);
      setMicMonitoring(false);
      setMicTestState('error');
      setMicTestError(error instanceof Error ? error.message : String(error));
      void stopMicrophoneLevelMonitor();
    }
  }, [copy, microphone, prefs?.microphoneDeviceName, stopMicTest]);

  useEffect(() => {
    if (activeSlide !== 'mic') {
      stopMicTest();
      return;
    }
    if (microphoneReady && micTestState === 'idle' && !autoMicStartedRef.current) {
      autoMicStartedRef.current = true;
      void startMicTest();
    }
  }, [activeSlide, micTestState, microphoneReady, startMicTest, stopMicTest]);

  useEffect(() => {
    if (activeSlide !== 'shortcut') {
      setHotkeyTestState('idle');
      return;
    }
    if (shortcutConfigured) {
      setHotkeyTestState('listening');
    }
  }, [activeSlide, shortcutConfigured]);

  useEffect(() => {
    if (hotkeyTestState !== 'listening') return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (!shortcutBinding.primary) return;
      if (event.key === 'Escape') {
        setHotkeyTestState('idle');
        return;
      }
      if (matchesShortcutBinding(event, shortcutBinding)) {
        event.preventDefault();
        event.stopPropagation();
        setHotkeyTestState('matched');
        if (hotkeyResetTimerRef.current !== null) {
          window.clearTimeout(hotkeyResetTimerRef.current);
        }
        hotkeyResetTimerRef.current = window.setTimeout(() => setHotkeyTestState('listening'), 1500);
      } else if (!isModifierKey(event.key)) {
        setHotkeyTestState('missed');
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => {
      window.removeEventListener('keydown', onKeyDown, true);
      if (hotkeyResetTimerRef.current !== null) {
        window.clearTimeout(hotkeyResetTimerRef.current);
        hotkeyResetTimerRef.current = null;
      }
    };
  }, [hotkeyTestState, shortcutBinding]);

  const goNext = () => {
    const nextIndex = Math.min(SLIDES.length - 1, slideIndex + 1);
    if (nextIndex === slideIndex || motionPhase !== 'settled') return;
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (reduceMotion) {
      setSlideIndex(nextIndex);
      return;
    }
    setMotionPhase('leaving');
    window.setTimeout(() => {
      setSlideIndex(nextIndex);
      setMotionPhase('entering');
      window.setTimeout(() => setMotionPhase('settled'), 28);
    }, 125);
  };

  const requestMicrophone = async () => {
    setPermissionBusy('microphone');
    try {
      if (microphone === 'denied' || microphone === 'restricted') {
        await openSystemSettings('microphone');
      } else {
        const status = await requestMicrophonePermission();
        setMicrophone(status);
        if (status === 'denied' || status === 'restricted') {
          await openSystemSettings('microphone');
        }
      }
      await refreshStatus();
    } finally {
      setPermissionBusy(null);
    }
  };

  const requestAccessibility = async () => {
    setPermissionBusy('accessibility');
    try {
      await requestAccessibilityPermission();
      await openSystemSettings('accessibility');
      await refreshStatus();
    } finally {
      setPermissionBusy(null);
    }
  };

  const saveShortcut = async (binding: ShortcutBinding) => {
    await setDictationHotkey(binding);
    if (prefs) {
      await updatePrefs({ ...prefs, dictationHotkey: binding });
    }
    setShortcutEditorOpen(false);
    setHotkeyTestState('listening');
  };

  const saveAsr = async () => {
    if (!asrFormValid || asrSaveState === 'saving') return;
    setAsrSaveState('saving');
    try {
      await setActiveAsrProvider(VOLCENGINE_PROVIDER_ID);
      await Promise.all([
        setCredential('volcengine.app_key', asrForm.appKey.trim()),
        setCredential('volcengine.access_key', asrForm.accessKey.trim()),
        setCredential('volcengine.resource_id', VOLCENGINE_RESOURCE_ID),
      ]);
      if (prefs) {
        await updatePrefs({ ...prefs, activeAsrProvider: VOLCENGINE_PROVIDER_ID });
      }
      await refreshStatus();
      setAsrSaveState('saved');
    } catch (error) {
      console.warn('[onboarding] save ASR failed', error);
      setAsrSaveState('error');
    }
  };

  const complete = async (options?: OnboardingCompleteOptions) => {
    await ensureOnlineAsrDefault();
    const shouldUseRawStyle = !credentials?.llmConfigured;
    if (shouldUseRawStyle) {
      try {
        await setActiveStylePack(BUILTIN_RAW_STYLE_PACK_ID);
      } catch (error) {
        console.warn('[onboarding] failed to switch to raw style before entering app', error);
      }
    }
    await updatePrefs(current => ({
      ...current,
      ...(shouldUseRawStyle
        ? {
          defaultMode: 'raw' as const,
          enabledModes: ['raw' as const],
          activeStylePackId: BUILTIN_RAW_STYLE_PACK_ID,
        }
        : {}),
      onboardingVersion: REQUIRED_ONBOARDING_VERSION,
    }));
    try {
      window.localStorage.setItem(ONBOARDING_COMPLETE_KEY, '1');
    } catch {
      // Some embedded webviews can reject localStorage access.
    }
    stopMicTest();
    onComplete(options);
  };

  const openVolcengineDoc = () => {
    void openExternal(VOLCENGINE_SETUP_URL).catch(error => {
      console.warn('[onboarding] open Volcengine doc failed', error);
    });
  };

  const openVolcengineStep = (url: string) => {
    void openExternal(url).catch(error => {
      console.warn('[onboarding] open Volcengine setup step failed', error);
    });
  };

  const handlePrimary = () => {
    if (activeSlide === 'mic') {
      if (!microphoneReady) {
        void requestMicrophone();
        return;
      }
      goNext();
      return;
    }
    if (activeSlide === 'shortcut') {
      if (!shortcutReady) {
        void requestAccessibility();
        return;
      }
      goNext();
      return;
    }
    if (activeSlide === 'asr') {
      if (asrReady) {
        void complete();
        return;
      }
      if (!asrHasInput) {
        openVolcengineDoc();
        return;
      }
      void saveAsr();
    }
  };

  const primaryLabel = (() => {
    if (activeSlide === 'mic') {
      if (!microphoneReady) return copy('onboarding.actionGrant', '允许');
      return copy('onboarding.next', '继续');
    }
    if (activeSlide === 'shortcut') {
      if (!shortcutReady) return copy('onboarding.actionGrant', '允许');
      return copy('onboarding.next', '继续');
    }
    if (asrReady) return copy('onboarding.enterAppReady', '开始使用');
    if (asrSaveState === 'saving') return copy('common.saving', '保存中...');
    if (!asrHasInput) return copy('onboarding.openVolcengineDoc', '打开火山文档');
    return copy('onboarding.saveAsr', '保存');
  })();

  const primaryDisabled = Boolean(
    permissionBusy
    || (activeSlide === 'asr' && asrHasInput && !asrFormValid)
    || (activeSlide === 'asr' && asrSaveState === 'saving')
  );

  const micStatus = micTestError
    ?? (micTestState === 'heard'
      ? copy('onboarding.micHeard', '已检测到声音。')
      : micTestState === 'listening'
        ? copy('onboarding.micListening', '说句话，正在听。')
        : microphoneReady
          ? copy('onboarding.micWaiting', '等待声音输入。')
          : copy('onboarding.micPermissionLabel', '等待麦克风权限。'));
  const micTone: StatusTone = micTestState === 'heard' ? 'success' : micTestState === 'error' ? 'warning' : 'neutral';

  const shortcutStatus = (() => {
    if (hotkeyStatus?.state === 'failed') {
      return copy('onboarding.hotkeyFailed', '快捷键监听异常。');
    }
    if (!shortcutConfigured) return copy('onboarding.shortcutNotSet', '先设置一个录音键。');
    if (!shortcutReady) return copy('onboarding.accessibilityTitle', '需要辅助功能权限。');
    if (hotkeyTestState === 'matched') return copy('onboarding.hig.shortcut.matched', '已确认。');
    if (hotkeyTestState === 'missed') return copy('onboarding.hig.shortcut.missed', '没有匹配，再按一次。');
    return copy('onboarding.hig.shortcut.listening', '按一次确认。');
  })();
  const shortcutTone: StatusTone = hotkeyTestState === 'matched' ? 'success' : hotkeyTestState === 'missed' || !shortcutReady ? 'warning' : 'neutral';

  const asrStatus = (() => {
    if (asrReady) return copy('onboarding.providerReady', '已连接。');
    if (asrSaveState === 'error') return copy('common.operationFailed', '保存失败，请检查密钥。');
    if (asrHasInput && !asrFormValid) return copy('onboarding.asrMissingFields', '还差一个字段。');
    return copy('onboarding.hig.asr.note', '推荐先用火山在线转写；其他服务稍后在设置中选择。');
  })();
  const asrTone: StatusTone = asrReady ? 'success' : asrSaveState === 'error' || (asrHasInput && !asrFormValid) ? 'warning' : 'neutral';
  const bodyStyle = isShortViewport ? shortWindowBodyStyle : isCompact ? compactWindowBodyStyle : windowBodyStyle;
  const stageStyle = {
    ...onboardingStageStyle,
    width: activeSlide === 'asr' && isCompact
      ? 'clamp(328px, calc(100vw - 272px), 520px)'
      : 'min(520px, calc(100vw - 24px))',
    transform: activeSlide === 'asr' && !isCompact ? 'translateX(-129px)' : 'none',
  };
  const windowFrameStyle = {
    ...(isCompact ? compactWindowStyle : windowStyle),
    width: '100%',
  };
  const guideDockStyle = isCompact ? compactVolcGuideDockStyle : volcGuideDockStyle;

  return (
    <div className="ol-onboarding-root" style={isCompact ? compactPageStyle : pageStyle}>
      <style>{`
        .ol-onboarding-root button:disabled {
          opacity: .48;
        }
        .ol-onboarding-slide {
          will-change: opacity, transform, filter;
        }
        .ol-motion-item {
          opacity: 1;
          transform: translate3d(0, 0, 0) scale(1);
          transition-property: opacity, transform;
          transition-duration: 260ms;
          transition-timing-function: cubic-bezier(0.22, 1, 0.36, 1);
          will-change: opacity, transform;
        }
        .ol-onboarding-slide-entering .ol-motion-item {
          opacity: 0;
          transform: translate3d(0, 8px, 0) scale(0.992);
        }
        .ol-onboarding-slide-leaving .ol-motion-item {
          opacity: 0;
          transform: translate3d(0, -5px, 0) scale(0.996);
          transition-duration: 120ms;
          transition-timing-function: cubic-bezier(0.4, 0, 0.7, 1);
        }
        .ol-onboarding-slide-settled .ol-motion-icon {
          transition-delay: 20ms;
        }
        .ol-onboarding-slide-settled .ol-motion-title {
          transition-delay: 42ms;
        }
        .ol-onboarding-slide-settled .ol-motion-desc {
          transition-delay: 58ms;
        }
        .ol-onboarding-slide-settled .ol-motion-operation {
          transition-delay: 76ms;
        }
        .ol-onboarding-slide-settled .ol-motion-status {
          transition-delay: 96ms;
        }
        .ol-onboarding-slide-leaving .ol-motion-item,
        .ol-onboarding-slide-entering .ol-motion-item {
          transition-delay: 0ms;
        }
        @media (prefers-reduced-motion: reduce) {
          .ol-onboarding-slide,
          .ol-motion-item {
            animation: none !important;
            transition: none !important;
            transform: none !important;
            filter: none !important;
          }
        }
      `}</style>

      <div style={stageStyle}>
        <section style={windowFrameStyle} aria-label={copy('onboarding.welcome', 'OpenLess 设置')}>
          <header style={windowBarStyle}>
            <div style={assistantTitleStyle}>
              <img src="AppIcon.png" alt="" style={assistantIconStyle} />
              <span style={assistantNameStyle}>OpenLess</span>
            </div>
            <StepDots slides={SLIDES} activeIndex={slideIndex} label={copy('onboarding.progress', '进度')} />
          </header>

          <main style={bodyStyle}>
            <div
              className={`ol-onboarding-slide ol-onboarding-slide-${motionPhase}`}
              style={{
                ...slideStyle,
                ...slideMotionStyles[motionPhase],
              }}
            >
              {activeSlide === 'mic' && (
                <SlideShell
                  icon="mic"
                  title={copy('onboarding.slideMicTitle', '试一下麦克风')}
                  desc={copy('onboarding.hig.mic.desc', '说句话，确认能听见。')}
                  dense={isShortViewport}
                >
                  <div className="ol-motion-item ol-motion-operation" style={operationStyle}>
                    <LevelMeter level={micLevel} active={micMonitoring} />
                  </div>
                  <StatusLine tone={micTone}>{micStatus}</StatusLine>
                </SlideShell>
              )}

              {activeSlide === 'shortcut' && (
                <SlideShell
                  icon="bolt"
                  title={copy('onboarding.slideShortcutTitle', '设置录音键')}
                  desc={copy('onboarding.hig.shortcut.desc', '选一个顺手的按键。')}
                  dense={isShortViewport}
                >
                  <div className="ol-motion-item ol-motion-operation" style={operationStyle}>
                    <div style={shortcutReadoutStyle}>
                      <span style={shortcutChipStyle}>{formatComboLabel(shortcutBinding)}</span>
                      <button
                        type="button"
                        style={quietButtonStyle}
                        onClick={() => setShortcutEditorOpen(v => !v)}
                        disabled={settingsLoading}
                      >
                        {shortcutEditorOpen
                          ? copy('onboarding.hig.shortcut.collapse', '收起')
                          : copy('onboarding.hig.shortcut.change', '更改')}
                      </button>
                    </div>
                    {shortcutEditorOpen && (
                      <div style={recorderShellStyle}>
                        <ShortcutRecorder
                          value={shortcutBinding}
                          onSave={saveShortcut}
                          alignRecordButton
                          disabled={settingsLoading}
                        />
                      </div>
                    )}
                  </div>
                  <StatusLine tone={shortcutTone}>{shortcutStatus}</StatusLine>
                </SlideShell>
              )}

              {activeSlide === 'asr' && (
                <SlideShell
                  icon="cloud"
                  title={copy('onboarding.hig.asr.title', '连接转写')}
                  desc={copy('onboarding.hig.asr.desc', '把语音变成文字。')}
                  dense={isShortViewport}
                >
                  <div className="ol-motion-item ol-motion-operation" style={asrOperationStyle}>
                    <div style={asrFieldGridStyle}>
                      <CredentialInput
                        label="APP ID"
                        value={asrForm.appKey}
                        onChange={value => {
                          setAsrSaveState('idle');
                          setAsrForm(v => ({ ...v, appKey: value }));
                        }}
                      />
                      <CredentialInput
                        label="Access Token"
                        value={asrForm.accessKey}
                        onChange={value => {
                          setAsrSaveState('idle');
                          setAsrForm(v => ({ ...v, accessKey: value }));
                        }}
                        secret
                      />
                    </div>
                    <div style={asrResourceNoteStyle}>
                      {copy('onboarding.hig.asr.resourceIdNote', 'Resource ID: volc.seedasr.sauc.duration')}
                    </div>
                  </div>
                  <StatusLine tone={asrTone}>{asrStatus}</StatusLine>
                </SlideShell>
              )}
            </div>
          </main>

          <footer style={windowFooterStyle}>
            {activeSlide === 'asr' && (
              <div style={secondaryButtonGroupStyle}>
                <button
                  type="button"
                  style={secondaryButtonStyle}
                  onClick={() => void complete({ openSettingsSection: 'providers' })}
                >
                  {copy('onboarding.hig.asr.otherOnline', '其他在线 ASR')}
                </button>
                <button
                  type="button"
                  style={secondaryButtonStyle}
                  onClick={() => void complete({ openSettingsSection: 'advanced' })}
                >
                  {copy('onboarding.hig.asr.localAi', '本地 AI')}
                </button>
              </div>
            )}
            <button type="button" style={primaryButtonStyle} onClick={handlePrimary} disabled={primaryDisabled}>
              {primaryLabel}
            </button>
          </footer>
        </section>

        {activeSlide === 'asr' && (
          <aside style={guideDockStyle} aria-label={copy('onboarding.hig.asr.volcGuide', '火山配置引导')}>
            <button
              type="button"
              aria-expanded={volcGuideOpen}
              style={volcGuideOpen ? volcGuideToggleActiveStyle : volcGuideToggleStyle}
              onClick={() => setVolcGuideOpen(open => !open)}
            >
              {copy('onboarding.hig.asr.volcGuide', '火山配置引导')}
            </button>
            {volcGuideOpen && (
              <div style={isCompact ? compactVolcGuidePanelStyle : volcGuidePanelStyle}>
            <div style={volcGuideHeaderStyle}>
              <div style={volcGuideTitleStyle}>
                {copy('onboarding.hig.asr.volcGuideTitle', 'OpenLess 火山 ASR 配置')}
              </div>
              <button
                type="button"
                aria-label={copy('common.close', '关闭')}
                style={volcGuideCloseStyle}
                onClick={() => setVolcGuideOpen(false)}
              >
                x
              </button>
            </div>
            <VolcGuideStep
              index={1}
              title={copy('onboarding.hig.asr.volcLogin', '登录火山引擎')}
              action={copy('onboarding.hig.asr.open', '打开')}
              compact={isCompact}
              onClick={() => openVolcengineStep(VOLCENGINE_LOGIN_URL)}
            />
            <VolcGuideStep
              index={2}
              title={copy('onboarding.hig.asr.volcCreateApp', '创建旧版应用')}
              desc={copy('onboarding.hig.asr.volcCreateAppDesc', '勾选豆包流式语音识别模型 2.0')}
              action={copy('onboarding.hig.asr.open', '打开')}
              compact={isCompact}
              onClick={() => openVolcengineStep(VOLCENGINE_APP_CREATE_URL)}
            />
            <VolcGuideStep
              index={3}
              title={copy('onboarding.hig.asr.volcService', '打开模型管理页')}
              desc={copy('onboarding.hig.asr.volcServiceDesc', '到底部复制 AppID 和 Access Token')}
              action={copy('onboarding.hig.asr.open', '打开')}
              compact={isCompact}
              onClick={() => openVolcengineStep(VOLCENGINE_SERVICE_URL)}
            />
            <div style={volcGuideFootnoteStyle}>
              {copy('onboarding.hig.asr.volcCopyBack', '复制后回到这里粘贴。Resource ID 使用 volc.seedasr.sauc.duration。')}
            </div>
              </div>
            )}
          </aside>
        )}
      </div>
    </div>
  );
}

function SlideShell({
  icon,
  title,
  desc,
  dense = false,
  children,
}: {
  icon: string;
  title: string;
  desc: string;
  dense?: boolean;
  children: ReactNode;
}) {
  return (
    <div style={slideInnerStyle}>
      <span className="ol-motion-item ol-motion-icon" style={dense ? denseSlideIconStyle : slideIconStyle}>
        <Icon name={icon} size={dense ? 20 : 22} />
      </span>
      <h1 className="ol-motion-item ol-motion-title" style={dense ? denseTitleStyle : titleStyle}>{title}</h1>
      <p className="ol-motion-item ol-motion-desc" style={dense ? denseDescStyle : descStyle}>{desc}</p>
      {children}
    </div>
  );
}

function StepDots({
  slides,
  activeIndex,
  label,
}: {
  slides: SlideId[];
  activeIndex: number;
  label: string;
}) {
  return (
    <div style={dotsStyle} aria-label={label}>
      {slides.map((slide, index) => (
        <span
          key={slide}
          style={{
            width: index === activeIndex ? 18 : 6,
            height: 6,
            borderRadius: 999,
            background: index === activeIndex
              ? 'var(--ol-ink, #1d1d1f)'
              : index < activeIndex
                ? 'rgba(0,0,0,0.30)'
                : 'rgba(0,0,0,0.13)',
            transform: index === activeIndex ? 'scale(1)' : 'scale(0.92)',
            transformOrigin: 'center',
            transition: 'width 0.34s cubic-bezier(0.22, 1, 0.36, 1), transform 0.34s cubic-bezier(0.22, 1, 0.36, 1), background 0.2s ease',
          }}
        />
      ))}
    </div>
  );
}

function StatusLine({ tone, children }: { tone: StatusTone; children: ReactNode }) {
  return (
    <div
      className="ol-motion-item ol-motion-status"
      style={{
        ...statusLineStyle,
        color: tone === 'success'
          ? 'var(--ol-ok, #248a3d)'
          : tone === 'warning'
            ? 'var(--ol-warn, #b45309)'
            : 'var(--ol-ink-4, #6e6e73)',
      }}
    >
      <span
        style={{
          ...statusDotStyle,
          background: tone === 'success'
            ? 'var(--ol-ok, #248a3d)'
            : tone === 'warning'
              ? 'var(--ol-warn, #b45309)'
              : 'rgba(0,0,0,0.22)',
        }}
      />
      {children}
    </div>
  );
}

function CredentialInput({
  label,
  value,
  onChange,
  secret = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  secret?: boolean;
}) {
  return (
    <label style={fieldStyle}>
      <span style={fieldLabelStyle}>{label}</span>
      <input
        type={secret ? 'password' : 'text'}
        value={value}
        onChange={event => onChange(event.currentTarget.value)}
        style={inputStyle}
      />
    </label>
  );
}

function VolcGuideStep({
  index,
  title,
  desc,
  action,
  compact = false,
  onClick,
}: {
  index: number;
  title: string;
  desc?: string;
  action: string;
  compact?: boolean;
  onClick: () => void;
}) {
  return (
    <div style={compact ? compactVolcGuideStepStyle : volcGuideStepStyle}>
      <span style={volcGuideIndexStyle}>{index}</span>
      <div style={volcGuideStepBodyStyle}>
        <div style={volcGuideStepTitleStyle}>{title}</div>
        {desc && <div style={volcGuideStepDescStyle}>{desc}</div>}
      </div>
      <button type="button" style={compact ? compactVolcGuideStepButtonStyle : volcGuideStepButtonStyle} onClick={onClick}>
        {action}
      </button>
    </div>
  );
}

function LevelMeter({ level, active }: { level: number; active: boolean }) {
  const amplified = Math.min(1, Math.max(0, level * 4.5));
  const bars = [0.28, 0.52, 0.82, 1, 0.82, 0.52, 0.28];
  return (
    <div style={levelMeterStyle} aria-hidden="true">
      {bars.map((weight, index) => {
        const idlePulse = active ? 0.08 : 0;
        const intensity = Math.min(1, amplified * (0.82 + weight * 0.36) + idlePulse);
        const height = 10 + intensity * (54 * weight);
        return (
          <span
            key={`${weight}-${index}`}
            style={{
              width: 8,
              height,
              borderRadius: 999,
              background: intensity > 0.1 ? 'var(--ol-blue, #007aff)' : 'rgba(0,0,0,0.13)',
              opacity: 0.35 + intensity * 0.65,
              transition: 'height 80ms linear, opacity 100ms ease, background 140ms ease',
            }}
          />
        );
      })}
    </div>
  );
}

function permissionReady(status: PermissionStatus) {
  return status === 'granted' || status === 'notApplicable';
}

function isLocalAsrProvider(provider: string | null | undefined) {
  return Boolean(provider && LOCAL_ASR_PROVIDER_IDS.has(provider));
}

function matchesShortcutBinding(event: KeyboardEvent, binding: ShortcutBinding) {
  const primary = primaryFromKeyboardEvent(event);
  if (!samePrimary(primary, binding.primary)) return false;
  const expected = new Set(binding.modifiers.map(m => m.toLowerCase()));
  const ownCtrl = primary === 'RightControl' || primary === 'LeftControl';
  const ownAlt = primary === 'RightOption' || primary === 'LeftOption';
  const ownShift = primary === 'Shift';
  const ownMeta = primary === 'RightCommand';
  return (
    event.ctrlKey === (expected.has('ctrl') || ownCtrl)
    && event.shiftKey === (expected.has('shift') || ownShift)
    && event.altKey === (expected.has('alt') || ownAlt)
    && event.metaKey === (expected.has('cmd') || expected.has('super') || ownMeta)
  );
}

function samePrimary(a: string, b: string) {
  if (a.length === 1 && b.length === 1) {
    return a.toLowerCase() === b.toLowerCase();
  }
  return a === b;
}

function primaryFromKeyboardEvent(event: KeyboardEvent) {
  if (isModifierKey(event.key)) {
    return modifierPrimaryFromCode(event.code, event.key);
  }
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3);
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5);
  if (event.code === 'Space') return 'Space';
  if (event.key.length === 1) return event.key;
  return event.key;
}

function modifierPrimaryFromCode(code: string, key: string) {
  if (key === 'Shift') return 'Shift';
  if (code === 'ControlRight') return 'RightControl';
  if (code === 'ControlLeft') return 'LeftControl';
  if (code === 'AltRight') return 'RightOption';
  if (code === 'AltLeft') return 'RightOption';
  if (code === 'MetaRight' || code === 'MetaLeft') return 'RightCommand';
  return key;
}

function isModifierKey(key: string) {
  return key === 'Control' || key === 'Alt' || key === 'Shift' || key === 'Meta';
}

const pageStyle: CSSProperties = {
  flex: 1,
  minHeight: '100dvh',
  display: 'grid',
  placeItems: 'center',
  padding: 24,
  boxSizing: 'border-box',
  background: '#f5f5f7',
  color: 'var(--ol-ink, #1d1d1f)',
  fontFamily: 'var(--ol-font-sans)',
};

const compactPageStyle: CSSProperties = {
  ...pageStyle,
  padding: 12,
};

const onboardingStageStyle: CSSProperties = {
  position: 'relative',
  width: 520,
};

const windowStyle: CSSProperties = {
  width: 520,
  height: 500,
  display: 'grid',
  gridTemplateRows: '56px minmax(0, 1fr) 64px',
  overflow: 'hidden',
  background: 'rgba(255,255,255,0.96)',
  border: '0.5px solid rgba(0,0,0,0.12)',
  borderRadius: 18,
  boxShadow: '0 28px 70px -46px rgba(0,0,0,0.45), 0 12px 32px -26px rgba(0,0,0,0.28)',
  position: 'relative',
};

const compactWindowStyle: CSSProperties = {
  ...windowStyle,
  width: 'calc(100vw - 24px)',
  height: 'calc(100dvh - 24px)',
  maxHeight: 560,
};

const windowBarStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr) auto',
  alignItems: 'center',
  padding: '0 18px',
  borderBottom: '0.5px solid rgba(0,0,0,0.07)',
  background: 'rgba(255,255,255,0.82)',
  backdropFilter: 'blur(18px)',
};

const assistantTitleStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 9,
  minWidth: 0,
};

const assistantIconStyle: CSSProperties = {
  width: 28,
  height: 28,
  borderRadius: 7,
  boxShadow: '0 1px 2px rgba(0,0,0,.12)',
};

const assistantNameStyle: CSSProperties = {
  fontSize: 13,
  fontWeight: 650,
  color: 'var(--ol-ink, #1d1d1f)',
};

const dotsStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-end',
  gap: 7,
};

const windowBodyStyle: CSSProperties = {
  minWidth: 0,
  minHeight: 0,
  display: 'grid',
  alignItems: 'center',
  padding: '28px 54px 18px',
  overflow: 'hidden',
};

const compactWindowBodyStyle: CSSProperties = {
  ...windowBodyStyle,
  padding: '24px 24px 16px',
};

const shortWindowBodyStyle: CSSProperties = {
  ...windowBodyStyle,
  padding: '18px 30px 10px',
};

const slideStyle: CSSProperties = {
  minWidth: 0,
  minHeight: 0,
  transition: 'opacity 0.24s cubic-bezier(0.22, 1, 0.36, 1), transform 0.24s cubic-bezier(0.22, 1, 0.36, 1), filter 0.18s ease',
};

const slideMotionStyles: Record<MotionPhase, CSSProperties> = {
  settled: {
    opacity: 1,
    transform: 'translate3d(0, 0, 0) scale(1)',
    filter: 'blur(0)',
  },
  leaving: {
    opacity: 0,
    transform: 'translate3d(0, -4px, 0) scale(0.995)',
    filter: 'blur(0.2px)',
  },
  entering: {
    opacity: 0,
    transform: 'translate3d(0, 8px, 0) scale(0.992)',
    filter: 'blur(0.2px)',
  },
};

const slideInnerStyle: CSSProperties = {
  minWidth: 0,
  display: 'grid',
  justifyItems: 'center',
  textAlign: 'center',
};

const slideIconStyle: CSSProperties = {
  width: 48,
  height: 48,
  borderRadius: 14,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: 'rgba(0,122,255,0.10)',
  color: 'var(--ol-blue, #007aff)',
  marginBottom: 18,
};

const denseSlideIconStyle: CSSProperties = {
  ...slideIconStyle,
  width: 42,
  height: 42,
  borderRadius: 12,
  marginBottom: 12,
};

const titleStyle: CSSProperties = {
  margin: 0,
  fontSize: 25,
  lineHeight: 1.18,
  fontWeight: 680,
  letterSpacing: 0,
};

const denseTitleStyle: CSSProperties = {
  ...titleStyle,
  fontSize: 22,
};

const descStyle: CSSProperties = {
  margin: '9px 0 0',
  maxWidth: 320,
  fontSize: 14,
  lineHeight: 1.45,
  color: 'var(--ol-ink-3, #515154)',
};

const denseDescStyle: CSSProperties = {
  ...descStyle,
  marginTop: 6,
  fontSize: 13,
  lineHeight: 1.36,
};

const operationStyle: CSSProperties = {
  width: '100%',
  marginTop: 26,
  boxSizing: 'border-box',
};

const asrOperationStyle: CSSProperties = {
  ...operationStyle,
  marginTop: 18,
};

const levelMeterStyle: CSSProperties = {
  height: 132,
  borderRadius: 14,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 9,
  background: 'rgba(0,122,255,0.06)',
  border: '0.5px solid rgba(0,122,255,0.14)',
};

const shortcutReadoutStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr) auto',
  alignItems: 'center',
  gap: 12,
};

const shortcutChipStyle: CSSProperties = {
  minWidth: 0,
  height: 74,
  borderRadius: 14,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: '0 18px',
  boxSizing: 'border-box',
  background: 'rgba(0,0,0,0.045)',
  color: 'var(--ol-ink, #1d1d1f)',
  fontFamily: 'var(--ol-font-mono)',
  fontSize: 22,
  fontWeight: 700,
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
};

const quietButtonStyle: CSSProperties = {
  height: 34,
  border: '0.5px solid rgba(0,0,0,0.14)',
  borderRadius: 8,
  background: '#fff',
  color: 'var(--ol-ink-2, #2c2c2e)',
  fontFamily: 'inherit',
  fontSize: 13,
  fontWeight: 650,
  padding: '0 12px',
  whiteSpace: 'nowrap',
  cursor: 'default',
};

const recorderShellStyle: CSSProperties = {
  marginTop: 12,
  padding: 12,
  borderRadius: 12,
  background: 'rgba(0,0,0,0.035)',
};

const asrFieldGridStyle: CSSProperties = {
  display: 'grid',
  gap: 9,
};

const asrResourceNoteStyle: CSSProperties = {
  marginTop: 8,
  fontSize: 11.5,
  lineHeight: 1.45,
  color: 'var(--ol-ink-4, #6e6e73)',
  textAlign: 'left',
  userSelect: 'text',
};

const volcGuideDockStyle: CSSProperties = {
  position: 'absolute',
  top: 82,
  left: 'calc(100% + 14px)',
  width: 244,
  zIndex: 20,
  display: 'grid',
  justifyItems: 'start',
  gap: 10,
};

const compactVolcGuideDockStyle: CSSProperties = {
  ...volcGuideDockStyle,
  top: 72,
  left: 'calc(100% + 10px)',
  width: 126,
};

const volcGuideToggleStyle: CSSProperties = {
  height: 32,
  border: '0.5px solid rgba(0,122,255,0.24)',
  borderRadius: 8,
  background: 'rgba(0,122,255,0.08)',
  color: 'var(--ol-blue, #007aff)',
  fontFamily: 'inherit',
  fontSize: 12.5,
  fontWeight: 650,
  padding: '0 12px',
  whiteSpace: 'nowrap',
  cursor: 'default',
};

const volcGuideToggleActiveStyle: CSSProperties = {
  ...volcGuideToggleStyle,
  background: 'var(--ol-blue, #007aff)',
  borderColor: 'var(--ol-blue, #007aff)',
  color: '#fff',
};

const volcGuidePanelStyle: CSSProperties = {
  width: 244,
  borderRadius: 14,
  background: 'rgba(255,255,255,0.96)',
  border: '0.5px solid rgba(0,0,0,0.12)',
  boxShadow: '0 18px 45px -28px rgba(0,0,0,0.48), 0 6px 18px -14px rgba(0,0,0,0.24)',
  backdropFilter: 'blur(20px) saturate(160%)',
  WebkitBackdropFilter: 'blur(20px) saturate(160%)',
  padding: 10,
  boxSizing: 'border-box',
};

const compactVolcGuidePanelStyle: CSSProperties = {
  ...volcGuidePanelStyle,
  width: 126,
  maxHeight: 'calc(100dvh - 124px)',
  overflowY: 'auto',
  overflowX: 'hidden',
  padding: 8,
};

const volcGuideHeaderStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 8,
  marginBottom: 7,
};

const volcGuideTitleStyle: CSSProperties = {
  minWidth: 0,
  fontSize: 12.5,
  fontWeight: 700,
  color: 'var(--ol-ink, #1d1d1f)',
};

const volcGuideCloseStyle: CSSProperties = {
  width: 24,
  height: 24,
  flex: '0 0 auto',
  border: 0,
  borderRadius: 999,
  background: 'rgba(0,0,0,0.045)',
  color: 'var(--ol-ink-4, #6e6e73)',
  fontFamily: 'inherit',
  fontSize: 13,
  lineHeight: 1,
  cursor: 'default',
};

const volcGuideStepStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '22px minmax(0, 1fr) auto',
  alignItems: 'center',
  gap: 8,
  padding: '8px 0',
  borderTop: '0.5px solid rgba(0,0,0,0.07)',
};

const compactVolcGuideStepStyle: CSSProperties = {
  ...volcGuideStepStyle,
  gridTemplateColumns: '20px minmax(0, 1fr)',
  alignItems: 'start',
  gap: 6,
  padding: '7px 0',
};

const volcGuideIndexStyle: CSSProperties = {
  width: 20,
  height: 20,
  borderRadius: 999,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: 'rgba(0,122,255,0.10)',
  color: 'var(--ol-blue, #007aff)',
  fontSize: 11,
  fontWeight: 700,
};

const volcGuideStepBodyStyle: CSSProperties = {
  minWidth: 0,
  textAlign: 'left',
};

const volcGuideStepTitleStyle: CSSProperties = {
  fontSize: 12,
  lineHeight: 1.25,
  fontWeight: 650,
  color: 'var(--ol-ink, #1d1d1f)',
};

const volcGuideStepDescStyle: CSSProperties = {
  marginTop: 2,
  fontSize: 11,
  lineHeight: 1.35,
  color: 'var(--ol-ink-4, #6e6e73)',
};

const volcGuideStepButtonStyle: CSSProperties = {
  height: 26,
  border: '0.5px solid rgba(0,0,0,0.14)',
  borderRadius: 7,
  background: '#fff',
  color: 'var(--ol-ink-2, #2c2c2e)',
  fontFamily: 'inherit',
  fontSize: 12,
  fontWeight: 650,
  padding: '0 9px',
  whiteSpace: 'nowrap',
  cursor: 'default',
};

const compactVolcGuideStepButtonStyle: CSSProperties = {
  ...volcGuideStepButtonStyle,
  gridColumn: '2',
  justifySelf: 'start',
  height: 24,
  marginTop: 4,
  fontSize: 11.5,
  padding: '0 8px',
};

const volcGuideFootnoteStyle: CSSProperties = {
  paddingTop: 8,
  borderTop: '0.5px solid rgba(0,0,0,0.07)',
  fontSize: 11,
  lineHeight: 1.45,
  color: 'var(--ol-ink-4, #6e6e73)',
  textAlign: 'left',
};

const fieldStyle: CSSProperties = {
  display: 'grid',
  gap: 6,
  textAlign: 'left',
};

const fieldLabelStyle: CSSProperties = {
  fontSize: 12,
  lineHeight: 1.2,
  fontWeight: 650,
  color: 'var(--ol-ink-4, #6e6e73)',
};

const inputStyle: CSSProperties = {
  width: '100%',
  height: 40,
  boxSizing: 'border-box',
  border: '0.5px solid rgba(0,0,0,0.18)',
  borderRadius: 8,
  background: '#fff',
  color: 'var(--ol-ink, #1d1d1f)',
  fontFamily: 'inherit',
  fontSize: 13,
  padding: '0 11px',
  outline: 'none',
};

const statusLineStyle: CSSProperties = {
  minHeight: 22,
  marginTop: 18,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 7,
  fontSize: 12.5,
  lineHeight: 1.35,
  fontWeight: 560,
};

const statusDotStyle: CSSProperties = {
  width: 6,
  height: 6,
  flex: '0 0 auto',
  borderRadius: 999,
};

const windowFooterStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-end',
  gap: 9,
  padding: '0 18px',
  borderTop: '0.5px solid rgba(0,0,0,0.07)',
  background: 'rgba(255,255,255,0.86)',
  backdropFilter: 'blur(18px)',
};

const secondaryButtonGroupStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  minWidth: 0,
};

const primaryButtonStyle: CSSProperties = {
  height: 34,
  minWidth: 104,
  border: 0,
  borderRadius: 8,
  background: 'var(--ol-blue, #007aff)',
  color: '#fff',
  fontFamily: 'inherit',
  fontSize: 13,
  fontWeight: 680,
  padding: '0 15px',
  whiteSpace: 'nowrap',
  cursor: 'default',
};

const secondaryButtonStyle: CSSProperties = {
  height: 34,
  minWidth: 76,
  border: '0.5px solid rgba(0,0,0,0.14)',
  borderRadius: 8,
  background: '#fff',
  color: 'var(--ol-ink-2, #2c2c2e)',
  fontFamily: 'inherit',
  fontSize: 13,
  fontWeight: 650,
  padding: '0 12px',
  whiteSpace: 'nowrap',
  cursor: 'default',
};
