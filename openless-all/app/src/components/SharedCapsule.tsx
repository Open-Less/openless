import { useTranslation } from 'react-i18next';
import { detectOS, type OS } from './WindowChrome';
import {
  getCapsuleHostMetrics,
  getCapsuleMessageLayout,
  getCapsulePillMetrics,
} from '../lib/capsuleLayout';
import type { CapsuleState } from '../lib/types';
import { useCapsuleState } from './useCapsuleState';

function AudioBars({ level }: { level: number }) {
  const envelope = [0.55, 0.85, 1.0, 0.85, 0.55];
  const base = 4;
  const max = 18;
  const voice = Math.min(1, Math.max(0, level));
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 3,
        width: 42,
        height: max,
      }}
    >
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
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 4,
        width: 24,
        height: 8,
      }}
    >
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

function CenterText({
  os,
  kind,
  text,
  color = 'var(--ol-ink-3)',
}: {
  os: OS;
  kind: 'default' | 'processing' | 'error';
  text: string;
  color?: string;
}) {
  const metrics = getCapsulePillMetrics(os);
  const layout = getCapsuleMessageLayout(os, kind);
  return (
    <span
      style={{
        fontSize: 11,
        fontWeight: 500,
        color,
        width: metrics.textWidth,
        textAlign: 'center',
        lineHeight: layout.allowWrap ? 1.2 : 1,
        whiteSpace: layout.allowWrap ? 'normal' : 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        display: '-webkit-box',
        WebkitBoxOrient: 'vertical',
        WebkitLineClamp: layout.lineClamp,
      }}
    >
      {text}
    </span>
  );
}

function CircleButton({
  variant,
  enabled,
  onClick,
}: {
  variant: 'cancel' | 'confirm';
  enabled: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  const isCancel = variant === 'cancel';
  return (
    <button
      onClick={enabled ? onClick : undefined}
      aria-label={isCancel ? t('common.cancel') : t('settings.shortcuts.confirm')}
      disabled={!enabled}
      style={{
        width: 28,
        height: 28,
        borderRadius: 999,
        background: isCancel ? 'rgba(255, 255, 255, 0.55)' : 'rgba(255, 255, 255, 0.92)',
        backdropFilter: isCancel ? 'blur(12px) saturate(160%)' : 'none',
        WebkitBackdropFilter: isCancel ? 'blur(12px) saturate(160%)' : 'none',
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

function SharedPill({
  os,
  state,
  level,
  insertedChars,
  message,
  onCancel,
  onConfirm,
}: {
  os: OS;
  state: CapsuleState;
  level: number;
  insertedChars: number;
  message?: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const metrics = getCapsulePillMetrics(os);
  const processingLayout = getCapsuleMessageLayout(os, 'processing');
  const enabled = state === 'recording';

  let center: JSX.Element;
  switch (state) {
    case 'recording':
      center = <AudioBars level={level} />;
      break;
    case 'transcribing':
    case 'polishing':
      center = (
        <div
          style={{
            display: 'inline-flex',
            flexDirection: 'row',
            alignItems: 'center',
            gap: 6,
            width: metrics.textWidth,
            justifyContent: 'center',
          }}
        >
          <ProcessingDots />
          <span
            style={{
              fontSize: 10.5,
              fontWeight: 500,
              color: 'var(--ol-ink-2)',
              textAlign: 'center',
              lineHeight: processingLayout.allowWrap ? 1.15 : 1,
              whiteSpace: processingLayout.allowWrap ? 'normal' : 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              display: '-webkit-box',
              WebkitBoxOrient: 'vertical',
              WebkitLineClamp: processingLayout.lineClamp,
            }}
          >
            {t('capsule.thinking')}
          </span>
        </div>
      );
      break;
    case 'done':
      center = <CenterText os={os} kind="default" text={message || t('capsule.inserted', { count: insertedChars })} />;
      break;
    case 'cancelled':
      center = <CenterText os={os} kind="default" text={t('capsule.cancelled')} />;
      break;
    case 'error':
      center = <CenterText os={os} kind="error" text={message || t('capsule.error')} color="var(--ol-err)" />;
      break;
    default:
      center = <AudioBars level={0} />;
  }

  const ambient = state === 'recording' ? Math.min(1, Math.max(0, level)) : 0;
  const scale = 1 + ambient * 0.018;
  const shadowAlpha = 0.20 + ambient * 0.10;

  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 8,
        padding: '0 8px',
        width: metrics.width,
        height: metrics.height,
        borderRadius: 999,
        background: 'rgba(255, 255, 255, 0.62)',
        backdropFilter: 'blur(28px) saturate(180%)',
        WebkitBackdropFilter: 'blur(28px) saturate(180%)',
        border: '1px solid rgba(255, 255, 255, 0.55)',
        boxShadow: `0 18px 50px -10px rgba(0, 0, 0, ${shadowAlpha.toFixed(3)}), 0 0 0 0.5px rgba(0, 0, 0, 0.08), inset 0 0.5px 0 rgba(255, 255, 255, 0.55)`,
        color: 'var(--ol-ink)',
        fontFamily: 'var(--ol-font-sans)',
        transform: `scale(${scale.toFixed(4)})`,
        transformOrigin: 'center',
        transition: 'transform 0.06s linear, box-shadow 0.06s linear',
        willChange: 'transform, box-shadow',
      }}
    >
      <CircleButton variant="cancel" enabled={enabled} onClick={onCancel} />
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        {center}
      </div>
      <CircleButton variant="confirm" enabled={enabled} onClick={onConfirm} />
    </div>
  );
}

export function SharedCapsule() {
  const os = detectOS();
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

  if (state === 'idle') {
    return <div style={{ width: 0, height: 0 }} />;
  }

  const metrics = getCapsulePillMetrics(os);
  const hostMetrics = getCapsuleHostMetrics(os, translation);

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        position: 'relative',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'transparent',
        animation: 'capsule-in .22s cubic-bezier(.2,.9,.3,1.1)',
      }}
    >
      <div
        style={{
          position: 'absolute',
          left: '50%',
          bottom: `${hostMetrics.bottomInset + metrics.height + hostMetrics.badgeGap}px`,
          transform: 'translateX(-50%)',
          pointerEvents: 'none',
        }}
      >
        <div
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 5,
            padding: '3px 10px',
            borderRadius: 999,
            fontSize: 10.5,
            fontWeight: 600,
            color: 'var(--ol-blue)',
            background: 'rgba(255, 255, 255, 0.78)',
            backdropFilter: 'blur(20px) saturate(180%)',
            WebkitBackdropFilter: 'blur(20px) saturate(180%)',
            border: '0.5px solid rgba(37, 99, 235, 0.25)',
            boxShadow: '0 4px 12px -4px rgba(37, 99, 235, 0.25), 0 0 0 0.5px rgba(0,0,0,0.04)',
            letterSpacing: '0.02em',
            whiteSpace: 'nowrap',
            opacity: translation ? 1 : 0,
            transform: translation ? 'translateY(0) scale(1)' : 'translateY(40px) scale(.88)',
            transformOrigin: 'center bottom',
            transition: 'opacity .24s ease-out, transform .34s cubic-bezier(.2,.9,.3,1.1)',
            willChange: 'opacity, transform',
          }}
        >
          <span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-blue)' }} />
          {t('capsule.translating')}
        </div>
      </div>
      <SharedPill
        os={os}
        state={state}
        level={level}
        insertedChars={insertedChars}
        message={message}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />
      <style>{`
        @keyframes capsule-in {
          from { opacity: 0; transform: translateY(6px) scale(.96); }
          to   { opacity: 1; transform: translateY(0) scale(1); }
        }
        @keyframes cap-dot {
          0%, 100% { opacity: 0.3; transform: scale(0.8); }
          50%      { opacity: 1.0; transform: scale(1.0); }
        }
      `}</style>
    </div>
  );
}
