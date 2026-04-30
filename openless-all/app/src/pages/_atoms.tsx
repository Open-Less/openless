// _atoms.tsx — shared display atoms used across the page bodies.
// Ported verbatim from design_handoff_openless/pages.jsx (PageHeader, Card,
// Pill, Btn). Inline styles preserved 1:1.

import type { CSSProperties, ReactNode } from 'react';
import { Icon } from '../components/Icon';

interface PageHeaderProps {
  kicker?: string;
  title: string;
  desc?: string;
  right?: ReactNode;
}

export function PageHeader({ kicker, title, desc, right }: PageHeaderProps) {
  return (
    <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 24, marginBottom: 24 }}>
      <div style={{ minWidth: 0 }}>
        {kicker && (
          <div style={{ fontSize: 11, fontWeight: 600, letterSpacing: '.08em', textTransform: 'uppercase', color: 'var(--ol-ink-4)', marginBottom: 8 }}>{kicker}</div>
        )}
        <h1 style={{ margin: 0, fontSize: 26, fontWeight: 600, letterSpacing: '-0.02em', color: 'var(--ol-ink)' }}>{title}</h1>
        {desc && <p style={{ margin: '8px 0 0', fontSize: 13, color: 'var(--ol-ink-3)', maxWidth: 640, lineHeight: 1.55 }}>{desc}</p>}
      </div>
      {right}
    </div>
  );
}

interface CardProps {
  children: ReactNode;
  style?: CSSProperties;
  padding?: number;
  glassy?: boolean;
}

export function Card({ children, style, padding = 18, glassy = false }: CardProps) {
  return (
    <div
      style={{
        background: glassy ? 'rgba(255,255,255,0.55)' : 'var(--ol-surface)',
        backdropFilter: glassy ? 'blur(20px) saturate(160%)' : undefined,
        WebkitBackdropFilter: glassy ? 'blur(20px) saturate(160%)' : undefined,
        border: '0.5px solid var(--ol-line)',
        borderRadius: 'var(--ol-r-lg)',
        padding,
        boxShadow: 'var(--ol-shadow-sm)',
        ...style,
      }}
    >
      {children}
    </div>
  );
}

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
    default: { bg: 'rgba(0,0,0,0.05)',   color: 'var(--ol-ink-2)',  bd: 'transparent' },
    blue:    { bg: 'var(--ol-blue-soft)',color: 'var(--ol-blue)',   bd: 'transparent' },
    ok:      { bg: 'var(--ol-ok-soft)',  color: 'var(--ol-ok)',     bd: 'transparent' },
    outline: { bg: 'transparent',        color: 'var(--ol-ink-3)',  bd: 'var(--ol-line-strong)' },
    dark:    { bg: 'var(--ol-ink)',      color: '#fff',             bd: 'transparent' },
  };
  const t = tones[tone];
  const sz = size === 'sm'
    ? { padding: '2px 8px', fontSize: 10.5 }
    : { padding: '4px 10px', fontSize: 11.5 };
  return (
    <span
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        borderRadius: 999,
        background: t.bg,
        color: t.color,
        border: t.bd === 'transparent' ? '0.5px solid transparent' : `0.5px solid ${t.bd}`,
        fontWeight: 500,
        ...sz,
        ...style,
      }}
    >
      {children}
    </span>
  );
}

export type BtnVariant = 'primary' | 'blue' | 'ghost' | 'soft';
export type BtnSize = 'sm' | 'md';

interface BtnProps {
  children: ReactNode;
  variant?: BtnVariant;
  size?: BtnSize;
  icon?: string;
  style?: CSSProperties;
  onClick?: () => void;
}

export function Btn({ children, variant = 'ghost', size = 'md', icon, style, onClick }: BtnProps) {
  const variants: Record<BtnVariant, { bg: string; color: string; bd: string; sh: string }> = {
    primary: { bg: 'var(--ol-ink)',     color: '#fff',                bd: 'transparent', sh: '0 1px 2px rgba(0,0,0,.08)' },
    blue:    { bg: 'var(--ol-blue)',    color: '#fff',                bd: 'transparent', sh: '0 1px 2px rgba(37,99,235,.18)' },
    ghost:   { bg: 'transparent',       color: 'var(--ol-ink-2)',     bd: 'var(--ol-line-strong)', sh: 'none' },
    soft:    { bg: 'rgba(0,0,0,0.04)',  color: 'var(--ol-ink-2)',     bd: 'transparent', sh: 'none' },
  };
  const v = variants[variant];
  const sizes: Record<BtnSize, { padding: string; fontSize: number }> = {
    sm: { padding: '5px 10px', fontSize: 12 },
    md: { padding: '7px 14px', fontSize: 12.5 },
  };
  return (
    <button
      onClick={onClick}
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        background: v.bg, color: v.color,
        border: v.bd === 'transparent' ? '0.5px solid transparent' : `0.5px solid ${v.bd}`,
        borderRadius: 8,
        boxShadow: v.sh,
        fontFamily: 'inherit', fontWeight: 500,
        cursor: 'default',
        ...sizes[size],
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={13} />}
      {children}
    </button>
  );
}
