// Onboarding.tsx — first-run permission and service setup.

import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { AndroidPermissionsPanel } from '@android/components/AndroidPermissionsPanel';
import { checkAndroidMicrophoneAccess, requestAndroidMicrophoneAccess } from '@android/lib/androidMicrophonePermission';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getPlatformCapabilities,
  openSystemSettings,
  requestAccessibilityPermission,
  requestMicrophonePermission,
  resetAccessibilityPermissionAndRestartApp,
} from '../lib/ipc';
import { getHotkeyTriggerLabel } from '../lib/hotkey';
import type { PermissionStatus, PlatformCapabilities } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { ProvidersSection } from '../pages/settings/ProvidersSection';

interface OnboardingProps {
  onComplete: () => void;
}

type AndroidStepId =
  | 'microphone'
  | 'accessibility'
  | 'overlayPermission'
  | 'overlayConfig'
  | 'asr'
  | 'llm';

export function Onboarding({ onComplete }: OnboardingProps) {
  const { t } = useTranslation();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  if (!platformCaps) {
    return <OnboardingLoading label={t('common.loading')} />;
  }

  if (platformCaps.platform === 'android') {
    return <AndroidOnboarding onComplete={onComplete} />;
  }

  return <DesktopOnboarding onComplete={onComplete} platformCaps={platformCaps} />;
}

function AndroidOnboarding({ onComplete }: OnboardingProps) {
  const { t } = useTranslation();
  const [stepIndex, setStepIndex] = useState(0);

  const steps = useMemo<Array<{ id: AndroidStepId; title: string; desc: string }>>(
    () => [
      {
        id: 'microphone',
        title: t('onboarding.androidSteps.microphoneTitle'),
        desc: t('onboarding.androidSteps.microphoneDesc'),
      },
      {
        id: 'accessibility',
        title: t('onboarding.androidSteps.accessibilityTitle'),
        desc: t('onboarding.androidSteps.accessibilityDesc'),
      },
      {
        id: 'overlayPermission',
        title: t('onboarding.androidSteps.overlayPermissionTitle'),
        desc: t('onboarding.androidSteps.overlayPermissionDesc'),
      },
      {
        id: 'overlayConfig',
        title: t('onboarding.androidSteps.overlayConfigTitle'),
        desc: t('onboarding.androidSteps.overlayConfigDesc'),
      },
      {
        id: 'asr',
        title: t('onboarding.androidSteps.asrTitle'),
        desc: t('onboarding.androidSteps.asrDesc'),
      },
      {
        id: 'llm',
        title: t('onboarding.androidSteps.llmTitle'),
        desc: t('onboarding.androidSteps.llmDesc'),
      },
    ],
    [t],
  );

  const current = steps[stepIndex] ?? steps[0];
  const isFirst = stepIndex === 0;
  const isLast = stepIndex === steps.length - 1;

  const goNext = () => {
    if (isLast) {
      onComplete();
      return;
    }
    setStepIndex((value) => Math.min(value + 1, steps.length - 1));
  };

  return (
    <OnboardingSurface>
      <div
        style={{
          width: '100%',
          maxWidth: 560,
          minHeight: '100%',
          display: 'flex',
          flexDirection: 'column',
          gap: 14,
        }}
      >
        <BrandHeader
          title={t('onboarding.androidTitle')}
          desc={t('onboarding.androidIntro')}
          compact
        />

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: `repeat(${steps.length}, 1fr)`,
            gap: 5,
            padding: '0 2px',
          }}
          aria-hidden
        >
          {steps.map((step, index) => (
            <div
              key={step.id}
              style={{
                height: 4,
                borderRadius: 999,
                background: index <= stepIndex ? 'var(--ol-blue)' : 'var(--ol-line-soft)',
              }}
            />
          ))}
        </div>

        <div
          style={{
            background: 'var(--ol-surface)',
            border: '0.5px solid var(--ol-line)',
            borderRadius: 14,
            boxShadow: 'var(--ol-shadow-lg)',
            padding: 18,
            display: 'flex',
            flexDirection: 'column',
            gap: 14,
            minWidth: 0,
          }}
        >
          <div>
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)', marginBottom: 4 }}>
              {t('onboarding.androidStepCounter', { current: stepIndex + 1, total: steps.length })}
            </div>
            <div style={{ fontSize: 17, fontWeight: 650 }}>{current.title}</div>
            <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55, marginTop: 5 }}>
              {current.desc}
            </div>
          </div>

          <AndroidStepContent step={current.id} />
        </div>

        <div style={{ display: 'flex', gap: 10, width: '100%' }}>
          <button
            type="button"
            onClick={() => setStepIndex((value) => Math.max(value - 1, 0))}
            disabled={isFirst}
            style={{
              ...secondaryButtonStyle,
              opacity: isFirst ? 0.45 : 1,
            }}
          >
            {t('onboarding.androidBack')}
          </button>
          <button type="button" onClick={goNext} style={primaryButtonStyle}>
            {isLast ? t('onboarding.androidFinish') : t('onboarding.androidNext')}
          </button>
        </div>

        <button type="button" onClick={onComplete} style={plainButtonStyle}>
          {t('onboarding.androidContinue')}
        </button>
      </div>
    </OnboardingSurface>
  );
}

function AndroidStepContent({ step }: { step: AndroidStepId }) {
  if (step === 'microphone') {
    return <AndroidMicrophoneStep />;
  }
  if (step === 'accessibility') {
    return <AndroidStepCard><AndroidPermissionsPanel mode="accessibility" /></AndroidStepCard>;
  }
  if (step === 'overlayPermission') {
    return <AndroidStepCard><AndroidPermissionsPanel mode="overlayPermission" /></AndroidStepCard>;
  }
  if (step === 'overlayConfig') {
    return <AndroidStepCard><AndroidPermissionsPanel mode="overlayConfig" /></AndroidStepCard>;
  }
  if (step === 'asr') {
    return <ProvidersSection kind="asr" />;
  }
  return <ProvidersSection kind="llm" />;
}

function AndroidMicrophoneStep() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<PermissionStatus>('notDetermined');
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    setStatus(await checkAndroidMicrophoneAccess());
  };

  useEffect(() => {
    void refresh();
    // issue #470：纯事件驱动，去掉高频轮询。窗口重新聚焦或重新可见时刷新（授权必经系统设置再切回）。
    const onFocus = () => { void refresh(); };
    const onVisibility = () => { if (document.visibilityState === 'visible') void refresh(); };
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, []);

  const request = async () => {
    setBusy(true);
    try {
      if (status === 'denied' || status === 'restricted') {
        await openSystemSettings('microphone');
      } else {
        setStatus(await requestAndroidMicrophoneAccess());
      }
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const granted = status === 'granted' || status === 'notApplicable';
  return (
    <AndroidStepCard>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
        <div>
          <div style={{ fontSize: 13, fontWeight: 600 }}>{t('onboarding.micTitle')}</div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.5, marginTop: 4 }}>
            {t('onboarding.micDesc')}
          </div>
        </div>
        <StatusBadge granted={granted} label={granted ? t('settings.permissions.granted') : t('settings.permissions.denied')} />
      </div>
      <button
        type="button"
        onClick={request}
        disabled={busy || granted}
        style={{
          ...primaryButtonStyle,
          width: '100%',
          opacity: busy || granted ? 0.55 : 1,
        }}
      >
        {granted ? t('onboarding.actionGranted') : t('onboarding.actionRequestMic')}
      </button>
    </AndroidStepCard>
  );
}

function DesktopOnboarding({
  onComplete,
  platformCaps: _platformCaps,
}: OnboardingProps & { platformCaps: PlatformCapabilities }) {
  const { t } = useTranslation();
  const [accessibility, setAccessibility] = useState<PermissionStatus>('notDetermined');
  const [microphone, setMicrophone] = useState<PermissionStatus>('notDetermined');
  const [busy, setBusy] = useState(false);
  // 区分「尚未尝试弹 TCC 对话框」和「TCC 已拒绝」两种 denied 状态。
  const [tccPromptShown, setTccPromptShown] = useState(false);
  const refreshTimeoutRef = useRef<number | null>(null);
  const { capability } = useHotkeySettings();

  const requiresAccessibility = !!capability?.requiresAccessibilityPermission;

  const refresh = async () => {
    const [a, m] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    setAccessibility(a);
    setMicrophone(m);
    // Gate on the real permission status — macOS returns granted/denied, other
    // platforms return notApplicable. This must NOT depend on the hotkey
    // capability flag: it is null while it loads, so trusting it let mic-only
    // onboarding finish before we knew macOS needs Accessibility, dropping users
    // into the app with a dead hotkey. Mirrors the gate in App.tsx.
    const aOk = a === 'granted' || a === 'notApplicable';
    const mOk = m === 'granted' || m === 'notApplicable';
    if (aOk && mOk) {
      onComplete();
    }
  };

  useEffect(() => {
    void refresh();
    // issue #470：纯事件驱动，去掉每秒轮询。授权必经系统设置 App，切回 OpenLess 必触发 focus/visibilitychange。
    const onFocus = () => { void refresh(); };
    const onVisibility = () => { if (document.visibilityState === 'visible') void refresh(); };
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
      if (refreshTimeoutRef.current) clearTimeout(refreshTimeoutRef.current);
    };
  }, [requiresAccessibility]);

  const onGrantAccessibility = async () => {
    setBusy(true);
    try {
      const result = await requestAccessibilityPermission();
      setTccPromptShown(true);
      // 如果 TCC 弹窗用户点了「允许」，权限已授予，不需要再打开系统设置。
      // 仅在 TCC 拒绝或之前已拒绝（不会再弹窗）时才打开系统设置引导用户手动开启。
      if (result !== 'granted') {
        await openSystemSettings('accessibility');
      }
    } finally {
      setBusy(false);
    }
    void refresh();
    if (refreshTimeoutRef.current) clearTimeout(refreshTimeoutRef.current);
    refreshTimeoutRef.current = window.setTimeout(refresh, 800);
  };

  const onRequestMicrophone = async () => {
    setBusy(true);
    try {
      if (microphone === 'denied') {
        await openSystemSettings('microphone');
      } else {
        const status = await requestMicrophonePermission();
        setMicrophone(status);
        if (status === 'denied' || status === 'restricted') {
          await openSystemSettings('microphone');
        }
      }
    } finally {
      setBusy(false);
    }
    if (refreshTimeoutRef.current) clearTimeout(refreshTimeoutRef.current);
    refreshTimeoutRef.current = window.setTimeout(refresh, 800);
  };

  return (
    <OnboardingSurface>
      <div
        style={{
          width: 'min(520px, 100%)',
          padding: 32,
          boxSizing: 'border-box',
          background: 'var(--ol-surface)',
          borderRadius: 14,
          border: '0.5px solid var(--ol-line)',
          boxShadow: 'var(--ol-shadow-lg)',
        }}
      >
        <BrandHeader title={t('onboarding.welcome')} desc={t('onboarding.intro')} />

        {(requiresAccessibility || accessibility === 'denied') && (
          <>
            <PermissionStep
              index={1}
              title={capability?.requiresAccessibilityPermission ? t('onboarding.accessibilityTitle') : t('onboarding.hotkeyTitle')}
              desc={capability?.requiresAccessibilityPermission
                ? t('onboarding.accessibilityDesc', { trigger: getHotkeyTriggerLabel(capability.availableTriggers[0]) })
                : capability?.statusHint ?? t('onboarding.hotkeyDesc')}
              status={accessibility}
              actionLabel={
                !capability?.requiresAccessibilityPermission || accessibility === 'notApplicable'
                  ? t('onboarding.actionNotApplicable')
                  : accessibility === 'granted'
                    ? t('onboarding.actionGranted')
                    : accessibility === 'denied' && tccPromptShown
                      ? t('onboarding.actionOpenSystem')
                      : t('onboarding.actionGrant')
              }
              onAction={onGrantAccessibility}
              disabled={busy || accessibility === 'granted' || accessibility === 'notApplicable'}
              hint={capability?.requiresAccessibilityPermission ? t('onboarding.accessibilityHint') : undefined}
            />
            {accessibility === 'denied' && tccPromptShown && (
              <div style={{ marginTop: 10, paddingTop: 10, borderTop: '0.5px solid var(--ol-line-soft)' }}>
                <button
                  type="button"
                  onClick={() => { resetAccessibilityPermissionAndRestartApp().catch(console.error); }}
                  style={{
                    width: '100%',
                    padding: '8px 14px',
                    fontSize: 12.5,
                    fontWeight: 500,
                    fontFamily: 'inherit',
                    border: '0.5px solid var(--ol-line-strong)',
                    borderRadius: 8,
                    background: 'var(--ol-surface)',
                    color: 'var(--ol-ink-2)',
                    cursor: 'default',
                  }}
                >
                  {t('onboarding.actionRestart')}
                </button>
              </div>
            )}
          </>
        )}

        <PermissionStep
          index={(requiresAccessibility || accessibility === 'denied') ? 2 : 1}
          title={t('onboarding.micTitle')}
          desc={t('onboarding.micDesc')}
          status={microphone}
          actionLabel={
            microphone === 'granted'
              ? t('onboarding.actionGranted')
              : microphone === 'denied'
                ? t('onboarding.actionOpenSystem')
                : t('onboarding.actionRequestMic')
          }
          onAction={onRequestMicrophone}
          disabled={busy || microphone === 'granted'}
        />

        <div style={footerHintStyle}>
          {t('onboarding.footerHint')}
        </div>
        <button
          type="button"
          onClick={onComplete}
          style={{
            width: '100%',
            marginTop: 12,
            padding: '9px 14px',
            fontSize: 12.5,
            fontWeight: 500,
            fontFamily: 'inherit',
            border: '0.5px solid var(--ol-line-strong)',
            borderRadius: 8,
            background: 'var(--ol-surface)',
            color: 'var(--ol-ink-2)',
            cursor: 'pointer',
          }}
        >
          {t('onboarding.continueToSettings')}
        </button>
      </div>
    </OnboardingSurface>
  );
}

function OnboardingLoading({ label }: { label: string }) {
  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontFamily: 'var(--ol-font-sans)',
        color: 'var(--ol-ink-3)',
        fontSize: 13,
      }}
    >
      {label}
    </div>
  );
}

function OnboardingSurface({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        overflow: 'auto',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 'calc(18px + env(safe-area-inset-top, 0px)) 16px calc(18px + env(safe-area-inset-bottom, 0px))',
        boxSizing: 'border-box',
        fontFamily: 'var(--ol-font-sans)',
      }}
    >
      {children}
    </div>
  );
}

function BrandHeader({ title, desc, compact = false }: { title: string; desc: string; compact?: boolean }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: compact ? 12 : 14, marginBottom: compact ? 4 : 18 }}>
      <img
        src="AppIcon.png"
        alt="OpenLess"
        style={{
          width: compact ? 48 : 52,
          height: compact ? 48 : 52,
          borderRadius: compact ? 12 : 13,
          flexShrink: 0,
        }}
      />
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: compact ? 17 : 18, fontWeight: 650 }}>{title}</div>
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', marginTop: 2, lineHeight: 1.45 }}>
          {desc}
        </div>
      </div>
    </div>
  );
}

function AndroidStepCard({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 12,
        padding: 14,
        borderRadius: 10,
        background: 'var(--ol-surface-2)',
        border: '0.5px solid var(--ol-line-soft)',
        minWidth: 0,
      }}
    >
      {children}
    </div>
  );
}

function StatusBadge({ granted, label }: { granted: boolean; label: string }) {
  return (
    <span
      style={{
        flexShrink: 0,
        fontSize: 11,
        fontWeight: 600,
        borderRadius: 999,
        padding: '4px 8px',
        color: granted ? 'var(--ol-ok)' : 'var(--ol-ink-4)',
        background: granted ? 'rgba(40, 160, 90, 0.12)' : 'rgba(0,0,0,0.06)',
      }}
    >
      {label}
    </span>
  );
}

interface StepProps {
  index: number;
  title: string;
  desc: string;
  status: PermissionStatus;
  actionLabel: string;
  onAction: () => void;
  disabled: boolean;
  hint?: string;
}

function PermissionStep({ index, title, desc, status, actionLabel, onAction, disabled, hint }: StepProps) {
  const granted = status === 'granted' || status === 'notApplicable';
  return (
    <div
      style={{
        padding: '14px 0',
        borderTop: '0.5px solid var(--ol-line-soft)',
        display: 'flex',
        gap: 14,
        alignItems: 'flex-start',
      }}
    >
      <div
        style={{
          width: 22,
          height: 22,
          borderRadius: 999,
          background: granted ? 'var(--ol-blue)' : 'rgba(0,0,0,0.06)',
          color: granted ? '#fff' : 'var(--ol-ink-3)',
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          fontSize: 11,
          fontWeight: 600,
          flexShrink: 0,
        }}
      >
        {granted ? '✓' : index}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13.5, fontWeight: 600 }}>{title}</div>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 3, lineHeight: 1.5 }}>{desc}</div>
        {hint && (
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>
            {hint.split('**').map((seg, i) => (i % 2 === 0 ? seg : <b key={i} style={{ color: 'var(--ol-ink-2)' }}>{seg}</b>))}
          </div>
        )}
      </div>
      <button
        onClick={disabled ? undefined : onAction}
        disabled={disabled}
        style={{
          flexShrink: 0,
          padding: '7px 14px',
          fontSize: 12.5,
          fontWeight: 500,
          fontFamily: 'inherit',
          border: 0,
          borderRadius: 8,
          background: granted ? 'var(--ol-surface-2)' : 'var(--ol-primary-solid-bg)',
          color: granted ? 'var(--ol-ink-3)' : 'var(--ol-primary-solid-ink)',
          cursor: disabled ? 'not-allowed' : 'default',
          opacity: disabled && !granted ? 0.6 : 1,
          transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), opacity 0.18s var(--ol-motion-soft), transform 0.12s var(--ol-motion-quick)',
        }}
      >
        {actionLabel}
      </button>
    </div>
  );
}

const primaryButtonStyle = {
  flex: 1,
  minHeight: 42,
  padding: '10px 14px',
  fontSize: 13,
  fontWeight: 600,
  fontFamily: 'inherit',
  border: 0,
  borderRadius: 10,
  background: 'var(--ol-primary-solid-bg)',
  color: 'var(--ol-primary-solid-ink)',
  cursor: 'default',
} as const;

const secondaryButtonStyle = {
  flex: 1,
  minHeight: 42,
  padding: '10px 14px',
  fontSize: 13,
  fontWeight: 600,
  fontFamily: 'inherit',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 10,
  background: 'var(--ol-surface)',
  color: 'var(--ol-ink-2)',
  cursor: 'default',
} as const;

const plainButtonStyle = {
  width: '100%',
  padding: '10px 14px',
  fontSize: 12.5,
  fontWeight: 500,
  fontFamily: 'inherit',
  border: 0,
  borderRadius: 8,
  background: 'transparent',
  color: 'var(--ol-ink-4)',
  cursor: 'default',
} as const;

const footerHintStyle = {
  marginTop: 18,
  padding: '12px 14px',
  borderRadius: 8,
  background: 'var(--ol-surface-2)',
  fontSize: 11.5,
  color: 'var(--ol-ink-3)',
  lineHeight: 1.6,
} as const;
