// Onboarding.tsx — 首次运行权限引导。
//
// 触发条件：App.tsx 启动检查 accessibility + microphone，任一未授权则渲染本组件而非主 Shell。
// 与 Swift `Sources/OpenLessApp/Onboarding/` 同语义，但简化为单页三步。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getPlatformCapabilities,
  openSystemSettings,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from '../lib/ipc';
import { getHotkeyTriggerLabel } from '../lib/hotkey';
import type { PermissionStatus, PlatformCapabilities } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';

interface OnboardingProps {
  onComplete: () => void;
}

export function Onboarding({ onComplete }: OnboardingProps) {
  const { t } = useTranslation();
  const [accessibility, setAccessibility] = useState<PermissionStatus>('notDetermined');
  const [microphone, setMicrophone] = useState<PermissionStatus>('notDetermined');
  const [busy, setBusy] = useState(false);
  const refreshTimeoutRef = useRef<number | null>(null);
  const { capability } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const isAndroid = platformCaps?.platform === 'android';
  const requiresAccessibility =
    !isAndroid && !!capability?.requiresAccessibilityPermission;

  const refresh = async () => {
    const [a, m] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    setAccessibility(a);
    setMicrophone(m);
    const aOk = !requiresAccessibility || a === 'granted' || a === 'notApplicable';
    const mOk = m === 'granted' || m === 'notApplicable';
    if (aOk && mOk) {
      onComplete();
    }
  };

  useEffect(() => {
    if (!platformCaps) return;
    refresh();
    const id = window.setInterval(refresh, 1000);
    // 用户从系统设置切回来时立刻刷新
    const onFocus = () => refresh();
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(id);
      window.removeEventListener('focus', onFocus);
      if (refreshTimeoutRef.current) clearTimeout(refreshTimeoutRef.current);
    };
  }, [isAndroid, requiresAccessibility]);

  const onGrantAccessibility = async () => {
    setBusy(true);
    try {
      await requestAccessibilityPermission();
      await openSystemSettings('accessibility');
    } finally {
      setBusy(false);
    }
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

  if (!platformCaps) {
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
        {t('common.loading')}
      </div>
    );
  }

  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 40,
        fontFamily: 'var(--ol-font-sans)',
      }}
    >
      <div
        style={{
          width: 520,
          padding: 32,
          background: 'var(--ol-surface)',
          borderRadius: 14,
          border: '0.5px solid var(--ol-line)',
          boxShadow: 'var(--ol-shadow-lg)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 18 }}>
          <div
            style={{
              width: 52,
              height: 52,
              borderRadius: 13,
              background: 'linear-gradient(135deg, #0a0a0b 0%, #2563eb 100%)',
              color: '#fff',
              fontSize: 22,
              fontWeight: 700,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            OL
          </div>
          <div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>{t('onboarding.welcome')}</div>
            <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', marginTop: 2 }}>
              {t('onboarding.intro')}
            </div>
          </div>
        </div>

        {requiresAccessibility && (
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
              : accessibility === 'denied'
                ? t('onboarding.actionOpenSystem')
                : t('onboarding.actionGrant')
          }
          onAction={onGrantAccessibility}
          disabled={busy || !capability?.requiresAccessibilityPermission || accessibility === 'granted' || accessibility === 'notApplicable'}
          hint={capability?.requiresAccessibilityPermission ? t('onboarding.accessibilityHint') : undefined}
        />
        )}

        <PermissionStep
          index={requiresAccessibility ? 2 : 1}
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

        {isAndroid && microphone !== 'granted' && microphone !== 'notApplicable' && (
          <button
            type="button"
            onClick={onComplete}
            disabled={busy}
            style={{
              marginTop: 14,
              width: '100%',
              padding: '10px 14px',
              fontSize: 12.5,
              fontWeight: 500,
              fontFamily: 'inherit',
              border: '0.5px solid var(--ol-line-strong)',
              borderRadius: 8,
              background: 'var(--ol-surface-2)',
              color: 'var(--ol-ink-2)',
              cursor: busy ? 'not-allowed' : 'default',
            }}
          >
            {t('onboarding.androidContinue')}
          </button>
        )}

        <div
          style={{
            marginTop: 18,
            padding: '12px 14px',
            borderRadius: 8,
            background: 'var(--ol-surface-2)',
            fontSize: 11.5,
            color: 'var(--ol-ink-3)',
            lineHeight: 1.6,
          }}
        >
          {isAndroid ? t('onboarding.androidFooterHint') : t('onboarding.footerHint')}
        </div>
      </div>
    </div>
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
          background: granted ? 'var(--ol-surface-2)' : 'var(--ol-ink)',
          color: granted ? 'var(--ol-ink-3)' : '#fff',
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
