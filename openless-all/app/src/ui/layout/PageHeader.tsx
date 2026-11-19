import type { ReactNode } from 'react';
import { useMobileLayout } from '../../lib/useMobileLayout';

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
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        justifyContent: 'space-between',
        gap: mobile ? 12 : 24,
        marginBottom: compact ? 2 : mobile ? 16 : 24,
        flexWrap: 'wrap',
        flexShrink: 0,
      }}
    >
      <div style={{ minWidth: 0, flex: '1 1 240px' }}>
        {kicker && (
          <div
            style={{
              fontSize: 11,
              fontWeight: 'var(--ol-font-weight-semibold)',
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
              fontWeight: 'var(--ol-font-weight-semibold)',
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
              maxWidth: 640,
              lineHeight: 1.55,
            }}
          >
            {desc}
          </p>
        )}
      </div>
      {right && (
        <div
          className="ol-page-header-actions"
          style={{ maxWidth: '100%', minWidth: 0, display: 'flex', flexWrap: 'wrap', gap: 8 }}
        >
          {right}
        </div>
      )}
    </div>
  );
}
