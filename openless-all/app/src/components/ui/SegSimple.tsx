// SegSimple — segmented control used in the Settings modal sub-sections.

import { useState } from 'react';
import { segmentedTrackStyle } from '../../pages/settings/shared';

interface SegSimpleProps {
  options: string[];
  active: string;
}

export function SegSimple({ options, active }: SegSimpleProps) {
  const [v, setV] = useState(active);
  return (
    <div style={segmentedTrackStyle}>
      {options.map((o) => (
        <button
          key={o}
          onClick={() => setV(o)}
          style={{
            padding: '5px 12px', fontSize: 12, fontWeight: 500, border: 0, borderRadius: 6,
            fontFamily: 'inherit',
            background: v === o ? 'var(--ol-segmented-active-bg)' : 'transparent',
            color: v === o ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
            boxShadow: v === o ? 'var(--ol-segmented-active-shadow)' : 'none',
            cursor: 'default',
          }}
        >
          {o}
        </button>
      ))}
    </div>
  );
}
