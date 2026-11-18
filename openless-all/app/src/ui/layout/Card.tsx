import type { CSSProperties, ReactNode } from 'react';

export interface CardProps {
  children: ReactNode;
  style?: CSSProperties;
  padding?: number;
  glassy?: boolean;
  className?: string;
}

export function Card({ children, style, padding = 18, glassy = false, className }: CardProps) {
  return (
    <div
      className={['ol-card', className].filter(Boolean).join(' ')}
      data-ol-toggle-group=""
      style={{
        background: glassy ? 'var(--ol-glass-bg)' : 'var(--ol-surface-raised)',
        backdropFilter: glassy ? 'blur(20px) saturate(160%)' : undefined,
        WebkitBackdropFilter: glassy ? 'blur(20px) saturate(160%)' : undefined,
        border: 0,
        borderRadius: 'var(--ol-r-lg)',
        padding,
        boxShadow: 'none',
        ...style,
      }}
    >
      {children}
    </div>
  );
}
