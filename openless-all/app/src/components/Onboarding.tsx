// Onboarding.tsx — first-run permission and service setup.

import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { AndroidPermissionsPanel } from '@android/components/AndroidPermissionsPanel';
import {
  checkAndroidMicrophoneAccess,
  requestAndroidMicrophoneAccess,
} from '@android/lib/androidMicrophonePermission';
import { getPlatformCapabilities, openSystemSettings } from '../lib/ipc';
import type { PermissionStatus, PlatformCapabilities } from '../lib/types';
import { ProvidersSection } from '../pages/settings/ChannelList';
import { DesktopOnboarding } from './DesktopOnboarding';

interface OnboardingProps {
  onComplete: () => void | Promise<void>;
}

type AndroidStepId =
  'microphone' | 'accessibility' | 'overlayPermission' | 'overlayConfig' | 'asr' | 'llm';

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

  return <DesktopOnboarding onComplete={onComplete} />;
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
            <div
              style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55, marginTop: 5 }}
            >
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
    return (
      <AndroidStepCard>
        <AndroidPermissionsPanel mode="accessibility" />
      </AndroidStepCard>
    );
  }
  if (step === 'overlayPermission') {
    return (
      <AndroidStepCard>
        <AndroidPermissionsPanel mode="overlayPermission" />
      </AndroidStepCard>
    );
  }
  if (step === 'overlayConfig') {
    return (
      <AndroidStepCard>
        <AndroidPermissionsPanel mode="overlayConfig" />
      </AndroidStepCard>
    );
  }
  if (step === 'asr') {
    return <ProvidersSection kind="asr" autoCreateWhenEmpty />;
  }
  return <ProvidersSection kind="llm" autoCreateWhenEmpty />;
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
    // issue #470: purely event-driven, no high-frequency polling. Refresh on window
    // refocus / re-visible (granting always goes through System Settings and back).
    const onFocus = () => {
      void refresh();
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') void refresh();
    };
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
      <div
        style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}
      >
        <div>
          <div style={{ fontSize: 13, fontWeight: 'var(--ol-font-weight-semibold)' }}>
            {t('onboarding.micTitle')}
          </div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.5, marginTop: 4 }}>
            {t('onboarding.micDesc')}
          </div>
        </div>
        <StatusBadge
          granted={granted}
          label={granted ? t('settings.permissions.granted') : t('settings.permissions.denied')}
        />
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
        padding:
          'calc(18px + env(safe-area-inset-top, 0px)) 16px calc(18px + env(safe-area-inset-bottom, 0px))',
        boxSizing: 'border-box',
        fontFamily: 'var(--ol-font-sans)',
      }}
    >
      {children}
    </div>
  );
}

function BrandHeader({
  title,
  desc,
  compact = false,
}: {
  title: string;
  desc: string;
  compact?: boolean;
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: compact ? 12 : 14,
        marginBottom: compact ? 4 : 18,
      }}
    >
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
        fontWeight: 'var(--ol-font-weight-semibold)',
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

const primaryButtonStyle = {
  flex: 1,
  minHeight: 42,
  padding: '10px 14px',
  fontSize: 13,
  fontWeight: 'var(--ol-font-weight-semibold)',
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
  fontWeight: 'var(--ol-font-weight-semibold)',
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
  fontWeight: 'var(--ol-font-weight-medium)',
  fontFamily: 'inherit',
  border: 0,
  borderRadius: 8,
  background: 'transparent',
  color: 'var(--ol-ink-4)',
  cursor: 'default',
} as const;
