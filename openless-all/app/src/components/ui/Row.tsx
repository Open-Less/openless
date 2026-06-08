// Row — two-column row used in the Settings modal sub-sections.

import type { ReactNode } from 'react';
import { useMobileLayout } from '../../lib/useMobileLayout';

interface RowProps {
  label: string;
  desc?: string;
  children: ReactNode;
}

export function Row({ label, desc, children }: RowProps) {
  const mobile = useMobileLayout();
  return (
    <div style={{ display: 'grid', gridTemplateColumns: mobile ? 'minmax(0, 1fr)' : '180px 1fr', gap: mobile ? 8 : 16, padding: '12px 0', borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</div>
        {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>{desc}</div>}
      </div>
      <div style={{ display: 'flex', alignItems: 'center', minWidth: 0, width: '100%', maxWidth: '100%', flexWrap: mobile ? 'wrap' : 'nowrap', gap: mobile ? 6 : undefined }}>{children}</div>
    </div>
  );
}
