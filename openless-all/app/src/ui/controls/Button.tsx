import type { CSSProperties, ReactNode } from 'react';
import { Icon } from '../../components/Icon';

export type ButtonVariant = 'primary' | 'blue' | 'ghost' | 'soft';
export type ButtonSize = 'sm' | 'md';

export interface ButtonProps {
  children?: ReactNode;
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: string;
  ariaLabel?: string;
  className?: string;
  ariaExpanded?: boolean;
  title?: string;
  style?: CSSProperties;
  onClick?: () => void;
  disabled?: boolean;
}

export function Button({
  children,
  variant = 'ghost',
  size = 'md',
  icon,
  ariaLabel,
  className,
  ariaExpanded,
  title,
  style,
  onClick,
  disabled = false,
}: ButtonProps) {
  const variants: Record<ButtonVariant, { bg: string; color: string; bd: string; sh: string }> = {
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
    ghost: { bg: 'var(--ol-surface-2)', color: 'var(--ol-ink-2)', bd: 'transparent', sh: 'none' },
    soft: {
      bg: 'var(--ol-control-muted)',
      color: 'var(--ol-ink-2)',
      bd: 'transparent',
      sh: 'none',
    },
  };
  const v = variants[variant];
  const sizes: Record<ButtonSize, { padding: string; fontSize: number }> = {
    sm: { padding: '5px 10px', fontSize: 13 },
    md: { padding: '7px 14px', fontSize: 13.5 },
  };
  // When the primary button is disabled, switch to light gray — translucent dark would still look clickable.
  const muted = disabled && (variant === 'primary' || variant === 'blue');
  return (
    <button
      type="button"
      className={['ol-pressable', 'ol-action-button', className].filter(Boolean).join(' ')}
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
        border: 0,
        borderRadius: 'var(--ol-button-radius)',
        boxShadow: muted ? 'none' : v.sh,
        fontFamily: 'inherit',
        fontWeight: 'var(--ol-font-weight-medium)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled && !muted ? 0.55 : 1,
        transition:
          'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft), transform 0.34s cubic-bezier(0.34, 1.56, 0.64, 1)',
        ...sizes[size],
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={13} />}
      {children}
    </button>
  );
}
