// _atoms.tsx — shared display atoms used across the page bodies.
// Ported verbatim from design_handoff_openless/pages.jsx (PageHeader, Card,
// Pill, Btn). Inline styles preserved 1:1.

import { useState, type CSSProperties, type ReactNode } from 'react';
import { Icon } from '../components/Icon';
import { useMobileLayout, useReadableLayout, useConservativeLayout } from '../lib/useMobileLayout';

interface PageHeaderProps {
  kicker?: string;
  title: string;
  desc?: string;
  right?: ReactNode;
  titleRight?: ReactNode;
  /** For single-screen pages like Overview: tightens the gap between the title and content below. */
  compact?: boolean;
}

export function PageHeader({
  kicker,
  title,
  desc,
  right,
  titleRight,
  compact = false,
}: PageHeaderProps) {
  const mobile = useMobileLayout();
  const readable = useReadableLayout();
  const conservative = useConservativeLayout();
  const preferenceStack = readable || conservative;
  const stackLayout = mobile || preferenceStack;
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        justifyContent: 'space-between',
        gap: stackLayout ? 12 : mobile ? 12 : 24,
        marginBottom: compact ? 8 : stackLayout ? 16 : mobile ? 16 : 24,
        flexWrap: 'wrap',
        flexShrink: 0,
      }}
    >
      <div style={{ minWidth: 0, flex: '1 1 240px' }}>
        {kicker && (
          <div
            style={{
              fontSize: 11,
              fontWeight: 600,
              letterSpacing: '.08em',
              textTransform: 'uppercase',
              color: 'var(--ol-ink-4)',
              marginBottom: 8,
            }}
          >
            {kicker}
          </div>
        )}
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <h1
            style={{
              margin: 0,
              fontSize: mobile ? 22 : 26,
              fontWeight: 600,
              letterSpacing: '-0.02em',
              color: 'var(--ol-ink)',
            }}
          >
            {title}
          </h1>
          {titleRight}
        </div>
        {desc && (
          <p
            style={{
              margin: '8px 0 0',
              fontSize: 13,
              color: 'var(--ol-ink-3)',
              maxWidth: preferenceStack ? undefined : 640,
              lineHeight: 1.55,
            }}
          >
            {desc}
          </p>
        )}
      </div>
      {right &&
        (preferenceStack ? (
          <div
            className="ol-flex-row ol-flex-split ol-page-header-actions"
            style={{ width: '100%' }}
          >
            {right}
          </div>
        ) : (
          <div
            className="ol-page-header-actions"
            style={{ maxWidth: '100%', minWidth: 0, display: 'flex', flexWrap: 'wrap', gap: 8 }}
          >
            {right}
          </div>
        ))}
    </div>
  );
}

interface CardProps {
  children: ReactNode;
  style?: CSSProperties;
  padding?: number;
  glassy?: boolean;
  className?: string;
}

export function Card({ children, style, padding = 18, glassy = false, className }: CardProps) {
  return (
    <div
      className={className}
      style={{
        background: glassy ? 'var(--ol-glass-bg)' : 'var(--ol-surface)',
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
    default: { bg: 'var(--ol-pill-bg)', color: 'var(--ol-ink-2)', bd: 'transparent' },
    blue: { bg: 'var(--ol-pill-blue-bg)', color: 'var(--ol-blue)', bd: 'transparent' },
    ok: { bg: 'var(--ol-pill-ok-bg)', color: 'var(--ol-ok)', bd: 'transparent' },
    outline: { bg: 'transparent', color: 'var(--ol-ink-3)', bd: 'var(--ol-line-strong)' },
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
        borderRadius: 999,
        background: t.bg,
        color: t.color,
        border: t.bd === 'transparent' ? '0.5px solid transparent' : `0.5px solid ${t.bd}`,
        fontWeight: 500,
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

export type BtnVariant = 'primary' | 'blue' | 'ghost' | 'soft';
export type BtnSize = 'sm' | 'md';

interface BtnProps {
  children?: ReactNode;
  variant?: BtnVariant;
  size?: BtnSize;
  icon?: string;
  ariaLabel?: string;
  ariaExpanded?: boolean;
  title?: string;
  style?: CSSProperties;
  onClick?: () => void;
  disabled?: boolean;
}

export function Btn({
  children,
  variant = 'ghost',
  size = 'md',
  icon,
  ariaLabel,
  ariaExpanded,
  title,
  style,
  onClick,
  disabled = false,
}: BtnProps) {
  const variants: Record<BtnVariant, { bg: string; color: string; bd: string; sh: string }> = {
    primary: {
      bg: 'var(--ol-primary-solid-bg)',
      color: 'var(--ol-primary-solid-ink)',
      bd: 'transparent',
      sh: 'var(--ol-shadow-sm)',
    },
    blue: {
      bg: 'var(--ol-accent-solid-bg)',
      color: 'var(--ol-accent-solid-ink)',
      bd: 'transparent',
      sh: 'var(--ol-shadow-sm)',
    },
    ghost: { bg: 'transparent', color: 'var(--ol-ink-2)', bd: 'var(--ol-line-strong)', sh: 'none' },
    soft: {
      bg: 'var(--ol-control-muted)',
      color: 'var(--ol-ink-2)',
      bd: 'transparent',
      sh: 'none',
    },
  };
  const v = variants[variant];
  const sizes: Record<BtnSize, { padding: string; fontSize: number }> = {
    sm: { padding: '5px 10px', fontSize: 13 },
    md: { padding: '7px 14px', fontSize: 13.5 },
  };
  // When the primary button is disabled, switch to light gray — translucent dark would still look clickable.
  const muted = disabled && (variant === 'primary' || variant === 'blue');
  return (
    <button
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      aria-label={ariaLabel}
      aria-expanded={ariaExpanded}
      title={title}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        background: muted ? 'var(--ol-control-muted)' : v.bg,
        color: muted ? 'var(--ol-ink-3)' : v.color,
        border: v.bd === 'transparent' ? '0.5px solid transparent' : `0.5px solid ${v.bd}`,
        borderRadius: 8,
        boxShadow: muted ? 'none' : v.sh,
        fontFamily: 'inherit',
        fontWeight: 500,
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled && !muted ? 0.55 : 1,
        transition:
          'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
        ...sizes[size],
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={13} />}
      {children}
    </button>
  );
}

interface CollapsibleProps {
  /// Title row content (short text, left-aligned).
  title: ReactNode;
  /// Optional subtitle / description (small text below the title).
  desc?: ReactNode;
  /// Whether expanded by default. Default false (collapsed, matching the "title + right arrow only" default).
  defaultOpen?: boolean;
  /// Set true when nested in a Card padding=0 container: removes top/bottom margins, relying only on the Card's borderBottom to separate.
  embedded?: boolean;
  children: ReactNode;
}

/// Collapsible section: collapsed by default, a `›` arrow on the right of the title row toggles
/// expand/collapse. The arrow rotates 90° when expanded. The content area transitions via
/// `grid-template-rows: 0fr ↔ 1fr` — the browser resolves `1fr` to the content's actual height and
/// transitions to it, avoiding the janky feel of a max-height animation where short content still plays
/// the full animation / closes with a delay. Requires Chromium 117+ (bundled with Tauri); modern
/// versions are fully supported.
///
/// `embedded=true`: for nesting inside a `<Card padding={0}>` sharing one Card with other Collapsibles;
/// adds a 0.5px bottom separator.
/// `embedded=false`: standalone block with the Card look (border / radius / shadow).
export function Collapsible({
  title,
  desc,
  defaultOpen = false,
  embedded = false,
  children,
}: CollapsibleProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div
      style={{
        borderBottom: embedded ? '0.5px solid var(--ol-line)' : undefined,
        border: embedded ? undefined : '0.5px solid var(--ol-line)',
        borderRadius: embedded ? 0 : 'var(--ol-r-lg)',
        background: embedded ? 'transparent' : 'var(--ol-surface)',
        boxShadow: embedded ? 'none' : 'var(--ol-shadow-sm)',
        overflow: 'hidden',
        // In a parent flex column with minHeight:0 + overflow:auto, flex children default to shrink:1
        // and the header button would be squeezed to a line. Lock it from shrinking; overflow scrolls in the parent.
        flexShrink: 0,
      }}
    >
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        // Announce the collapsed state to screen readers; keyboard focus keeps the browser's default outline
        // instead of outline: 'none' so Tab navigation keeps a visual focus indicator (pr-agent #407).
        style={{
          width: '100%',
          padding: '14px 18px',
          background: 'transparent',
          border: 0,
          textAlign: 'left',
          fontFamily: 'inherit',
          color: 'inherit',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          cursor: 'pointer',
        }}
      >
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>{title}</div>
          {desc && (
            <div
              style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 3, lineHeight: 1.5 }}
            >
              {desc}
            </div>
          )}
        </div>
        <span
          aria-hidden="true"
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            width: 18,
            height: 18,
            color: 'var(--ol-ink-4)',
            transform: open ? 'rotate(90deg)' : 'rotate(0deg)',
            transition: 'transform 0.18s var(--ol-motion-quick)',
          }}
        >
          <Icon name="chevRight" size={14} />
        </span>
      </button>
      <div
        style={{
          display: 'grid',
          // grid-template-rows: 0fr → 1fr makes the browser resolve 1fr to the content's actual height.
          gridTemplateRows: open ? '1fr' : '0fr',
          transition: 'grid-template-rows 0.22s var(--ol-motion-soft)',
        }}
        // inert removes internal interactive elements from tab order + the a11y tree so keyboard users
        // can't tab into invisible inputs / buttons / toggles after collapsing (pr-agent #407).
        // Support: Chromium 102+ (the Tauri WebView is far newer) / Safari 15.4+.
        // React 18 types don't accept `inert`; pass the string-boolean via spread to bypass the compiler.
        {...(!open ? { inert: '' } : {})}
        aria-hidden={!open}
      >
        {/* minHeight: 0 is required: a grid item won't shrink below its content's intrinsic height by default;
            without this trick nothing animates and the row-height animation fails too. */}
        <div style={{ overflow: 'hidden', minHeight: 0 }}>
          <div style={{ padding: '0 18px 18px' }}>{children}</div>
        </div>
      </div>
    </div>
  );
}
