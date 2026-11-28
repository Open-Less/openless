import type { CSSProperties, ReactNode } from 'react';

export type PillTone = 'default' | 'blue' | 'ok' | 'outline' | 'dark';
export type PillSize = 'sm' | 'md';

interface PillProps {
  children: ReactNode;
  tone?: PillTone;
  size?: PillSize;
  style?: CSSProperties;
}

export function Pill({ children, tone = 'default', size = 'md', style }: PillProps) {
  const tones: Record<PillTone, { bg: string; color: string; bd: string }> = {
    default: { bg: 'var(--ol-pill-bg)', color: 'var(--ol-ink-2)', bd: 'transparent' },
    blue: { bg: 'var(--ol-pill-blue-bg)', color: 'var(--ol-blue)', bd: 'transparent' },
    ok: { bg: 'var(--ol-pill-ok-bg)', color: 'var(--ol-ok)', bd: 'transparent' },
    outline: { bg: 'var(--ol-surface-2)', color: 'var(--ol-ink-3)', bd: 'transparent' },
    dark: {
      bg: 'var(--ol-pill-selected-bg)',
      color: 'var(--ol-pill-selected-ink)',
      bd: 'transparent',
    },
  };
  const t = tones[tone];
  const sz =
    size === 'sm'
      ? { padding: '2px 8px', fontSize: 10.5 }
      : { padding: '4px 10px', fontSize: 11.5 };
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        borderRadius: 'var(--ol-radius)',
        background: t.bg,
        color: t.color,
        border: 0,
        fontWeight: 'var(--ol-font-weight-medium)',
        whiteSpace: 'nowrap',
        flexShrink: 0,
        ...sz,
        ...style,
      }}
    >
      {children}
    </span>
  );
}
