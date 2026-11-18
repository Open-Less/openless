import type { ReactNode } from 'react';

export function ExperimentalSectionLabel({
  children,
  badge,
}: {
  children: ReactNode;
  badge: string;
}) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7, flexWrap: 'wrap' }}>
      <span>{children}</span>
      <span
        style={{
          padding: '2px 6px',
          borderRadius: 'var(--ol-radius)',
          background: 'var(--ol-blue-soft)',
          color: 'var(--ol-blue)',
          fontSize: 10,
          fontWeight: 'var(--ol-font-weight-semibold)',
          lineHeight: 1.3,
          letterSpacing: 0,
        }}
      >
        {badge}
      </span>
    </span>
  );
}
