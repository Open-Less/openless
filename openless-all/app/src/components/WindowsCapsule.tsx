import { useTranslation } from 'react-i18next';
import {
  getCapsuleHostMetrics,
  getCapsulePillMetrics,
} from '../lib/capsuleLayout';
import type { CapsuleState } from '../lib/types';
import { useCapsuleState } from './useCapsuleState';

interface AudioBarsProps {
  level: number;
}

function AudioBars({ level }: AudioBarsProps) {
  const envelope = [0.55, 0.85, 1.0, 0.85, 0.55];
  const base = 4;
  const max = 18;
  const voice = Math.min(1, Math.max(0, level));
  return (
    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 3, width: 42, height: max }}>
      {envelope.map((env, i) => (
        <span
          key={i}
          style={{
            display: 'inline-block',
            width: 3,
            height: base + (max - base) * voice * env,
            borderRadius: 999,
            background: 'var(--ol-blue)',
            opacity: 0.82,
            transition: 'height 0.06s linear',
          }}
        />
      ))}
    </div>
  );
}

function ProcessingDots() {
  return (
    <div style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 4, width: 24, height: 8 }}>
      {[0, 1, 2].map(i => (
        <span
          key={i}
          style={{
            width: 4,
            height: 4,
            borderRadius: 999,
            background: 'var(--ol-blue)',
            opacity: 0.85,
            animation: `cap-dot 0.9s linear ${i * 0.3}s infinite`,
          }}
        />
      ))}
    </div>
  );
}

function WindowsCapsuleButton({
  type,
  enabled,
  onClick,
}: {
  type: 'cancel' | 'confirm';
  enabled: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  const isCancel = type === 'cancel';
  return (
    <button
      onClick={enabled ? onClick : undefined}
      aria-label={isCancel ? t('common.cancel') : t('settings.shortcuts.confirm')}
      disabled={!enabled}
      style={{
        width: 28,
        height: 28,
        borderRadius: 999,
        background: isCancel ? 'rgba(255,255,255,0.90)' : 'rgba(255,255,255,0.96)',
        color: 'var(--ol-ink)',
        border: '0.8px solid rgba(0, 0, 0, 0.08)',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: enabled ? 'default' : 'not-allowed',
        opacity: enabled ? 1 : 0.42,
        flexShrink: 0,
        padding: 0,
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.06)',
      }}
    >
      {isCancel ? (
        <svg width="11" height="11" viewBox="0 0 11 11">
          <path d="M1.5 1.5l8 8M9.5 1.5l-8 8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      ) : (
        <svg width="13" height="13" viewBox="0 0 13 13">
          <path d="M2 6.5l3.2 3.5L11 3.5" stroke="currentColor" strokeWidth="1.7" fill="none" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}
    </button>
  );
}

function WindowsProcessingCenter({ text }: { text: string }) {
  const metrics = getCapsulePillMetrics('win');
  return (
    <div
      style={{
        width: metrics.textWidth,
        minHeight: 32,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 4,
      }}
    >
      <ProcessingDots />
      <span
        style={{
          width: '100%',
          fontSize: 11,
          fontWeight: 500,
          color: 'var(--ol-ink-2)',
          textAlign: 'center',
          lineHeight: 1.1,
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
        }}
      >
        {text}
      </span>
    </div>
  );
}

function WindowsCenterText({
  text,
  color = 'var(--ol-ink-2)',
}: {
  text: string;
  color?: string;
}) {
  const metrics = getCapsulePillMetrics('win');
  return (
    <span
      style={{
        width: metrics.textWidth,
        fontSize: 11,
        fontWeight: 500,
        color,
        textAlign: 'center',
        lineHeight: 1.15,
        whiteSpace: 'normal',
        display: '-webkit-box',
        WebkitBoxOrient: 'vertical',
        WebkitLineClamp: 2,
        overflow: 'hidden',
      }}
    >
      {text}
    </span>
  );
}

function WindowsCapsulePill({
  state,
  level,
  insertedChars,
  message,
  onCancel,
  onConfirm,
}: {
  state: CapsuleState;
  level: number;
  insertedChars: number;
  message?: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const metrics = getCapsulePillMetrics('win');
  const active = state === 'recording';

  let center: JSX.Element;
  switch (state) {
    case 'recording':
      center = <AudioBars level={level} />;
      break;
    case 'transcribing':
    case 'polishing':
      center = <WindowsProcessingCenter text={t('capsule.thinking')} />;
      break;
    case 'done':
      center = <WindowsCenterText text={message || t('capsule.inserted', { count: insertedChars })} />;
      break;
    case 'cancelled':
      center = <WindowsCenterText text={t('capsule.cancelled')} />;
      break;
    case 'error':
      center = <WindowsCenterText text={message || t('capsule.error')} color="var(--ol-err)" />;
      break;
    default:
      center = <AudioBars level={0} />;
  }

  return (
    <div
      style={{
        width: metrics.width,
        height: metrics.height,
        position: 'relative',
      }}
    >
      <div
        style={{
          position: 'absolute',
          inset: 0,
          borderRadius: 999,
          background: 'rgba(255, 255, 255, 0.92)',
          border: '1px solid rgba(0, 0, 0, 0.06)',
          boxShadow: '0 8px 24px -16px rgba(15,17,22,0.20), 0 0 0 0.5px rgba(0, 0, 0, 0.04), inset 0 0.5px 0 rgba(255,255,255,0.92)',
        }}
      />
      <div
        style={{
          position: 'relative',
          zIndex: 1,
          width: metrics.width,
          height: metrics.height,
        display: 'grid',
        gridTemplateColumns: '28px 1fr 28px',
        alignItems: 'center',
        columnGap: 10,
        padding: '0 12px',
        background: 'transparent',
        border: 'none',
        boxShadow: 'none',
        overflow: 'visible',
      }}
    >
      <WindowsCapsuleButton type="cancel" enabled={active} onClick={onCancel} />
      <div style={{ minWidth: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        {center}
      </div>
      <WindowsCapsuleButton type="confirm" enabled={active} onClick={onConfirm} />
      </div>
    </div>
  );
}

export function WindowsCapsule() {
  const { t } = useTranslation();
  const {
    state,
    level,
    insertedChars,
    message,
    translation,
    onCancel,
    onConfirm,
  } = useCapsuleState();
  const metrics = getCapsulePillMetrics('win');
  const hostMetrics = getCapsuleHostMetrics('win', translation);

  if (state === 'idle') {
    return <div style={{ width: 0, height: 0 }} />;
  }

  return (
    <div
      style={{
        width: hostMetrics.width,
        height: hostMetrics.height,
        position: 'relative',
        display: 'flex',
        alignItems: 'flex-end',
        justifyContent: 'center',
        paddingBottom: hostMetrics.bottomInset,
        background: 'transparent',
      }}
    >
      {translation && (
        <div
          style={{
            position: 'absolute',
            left: '50%',
            bottom: `${hostMetrics.bottomInset + metrics.height + hostMetrics.badgeGap}px`,
            transform: 'translateX(-50%)',
            pointerEvents: 'none',
            display: 'inline-flex',
            alignItems: 'center',
            gap: 5,
            padding: '3px 10px',
            borderRadius: 999,
            fontSize: 10.5,
            fontWeight: 600,
            color: 'var(--ol-blue)',
            background: 'rgba(255, 255, 255, 0.9)',
            border: '0.5px solid rgba(37, 99, 235, 0.18)',
            boxShadow: '0 4px 12px -4px rgba(37, 99, 235, 0.18)',
            whiteSpace: 'nowrap',
          }}
        >
          <span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-blue)' }} />
          {t('capsule.translating')}
        </div>
      )}
      <WindowsCapsulePill
        state={state}
        level={level}
        insertedChars={insertedChars}
        message={message}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />
      <style>{`
        @keyframes cap-dot {
          0%, 100% { opacity: 0.3; transform: scale(0.8); }
          50%      { opacity: 1.0; transform: scale(1.0); }
        }
      `}</style>
    </div>
  );
}
