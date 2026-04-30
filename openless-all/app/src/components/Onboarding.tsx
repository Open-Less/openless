// Onboarding.tsx — 首次运行权限引导。
//
// 触发条件：App.tsx 启动检查 accessibility + microphone，任一未授权则渲染本组件而非主 Shell。
// 与 Swift `Sources/OpenLessApp/Onboarding/` 同语义，但简化为单页三步。

import { useEffect, useState } from 'react';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  openSystemSettings,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from '../lib/ipc';
import { getHotkeyTriggerLabel } from '../lib/hotkey';
import type { PermissionStatus } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';

interface OnboardingProps {
  onComplete: () => void;
}

export function Onboarding({ onComplete }: OnboardingProps) {
  const [accessibility, setAccessibility] = useState<PermissionStatus>('notDetermined');
  const [microphone, setMicrophone] = useState<PermissionStatus>('notDetermined');
  const [busy, setBusy] = useState(false);
  const { capability } = useHotkeySettings();

  const refresh = async () => {
    const [a, m] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    setAccessibility(a);
    setMicrophone(m);
    if ((a === 'granted' || a === 'notApplicable') && (m === 'granted' || m === 'notApplicable')) {
      onComplete();
    }
  };

  useEffect(() => {
    refresh();
    const id = window.setInterval(refresh, 1000);
    // 用户从系统设置切回来时立刻刷新
    const onFocus = () => refresh();
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(id);
      window.removeEventListener('focus', onFocus);
    };
  }, []);

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
    setTimeout(refresh, 800);
  };

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
            <div style={{ fontSize: 18, fontWeight: 600 }}>欢迎使用 OpenLess</div>
            <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', marginTop: 2 }}>
              本地说出，本地落字。开始前需要两个系统权限。
            </div>
          </div>
        </div>

        <PermissionStep
          index={1}
          title={capability?.requiresAccessibilityPermission ? '辅助功能' : '全局快捷键'}
          desc={capability?.requiresAccessibilityPermission
            ? `用于监听全局快捷键（默认 ${getHotkeyTriggerLabel(capability.availableTriggers[0])}）并把识别结果写入光标位置。`
            : capability?.statusHint ?? '用于确认全局快捷键监听可用。'}
          status={accessibility}
          actionLabel={
            !capability?.requiresAccessibilityPermission || accessibility === 'notApplicable'
              ? '无需授权'
              : accessibility === 'granted'
              ? '已授权'
              : accessibility === 'denied'
                ? '打开系统设置'
                : '授权'
          }
          onAction={onGrantAccessibility}
          disabled={busy || !capability?.requiresAccessibilityPermission || accessibility === 'granted' || accessibility === 'notApplicable'}
          hint={capability?.requiresAccessibilityPermission ? '授权后必须**完全退出 OpenLess** 再重新打开（macOS TCC 规则）。' : undefined}
        />

        <PermissionStep
          index={2}
          title="麦克风"
          desc="用于捕获你的语音输入。"
          status={microphone}
          actionLabel={
            microphone === 'granted'
              ? '已授权'
              : microphone === 'denied'
                ? '打开系统设置'
                : '弹出授权'
          }
          onAction={onRequestMicrophone}
          disabled={busy || microphone === 'granted'}
        />

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
          授权全部完成后此引导自动关闭。如果一直不消失，从菜单栏 OpenLess → 退出，重新打开 App。
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
        }}
      >
        {actionLabel}
      </button>
    </div>
  );
}
