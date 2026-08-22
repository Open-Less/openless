// Row — two-column row used in the Settings modal sub-sections.

import type { ReactNode } from 'react';
import { useLayoutStack, useConservativeLayout } from '../../lib/useMobileLayout';

interface RowProps {
  label: string;
  desc?: string;
  children: ReactNode;
}

export function Row({ label, desc, children }: RowProps) {
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const stackLayout = baseLayoutStack || conservative;
  return (
    <div style={{
      display: 'grid',
      gridTemplateColumns: stackLayout ? 'minmax(0, 1fr)' : '180px minmax(0, 1fr)',
      gap: stackLayout ? 8 : 16,
      padding: '12px 0',
      borderTop: '0.5px solid var(--ol-line-soft)',
      alignItems: 'center',
    }}>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</div>
        {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>{desc}</div>}
      </div>
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'flex-start',
        minWidth: 0,
        width: stackLayout ? '100%' : 'auto',
        maxWidth: '100%',
        flexWrap: stackLayout ? 'wrap' : 'nowrap',
        gap: stackLayout ? 6 : undefined,
      }}>
        {children}
      </div>
    </div>
  );
}
