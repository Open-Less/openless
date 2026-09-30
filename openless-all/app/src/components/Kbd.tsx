// Kbd.tsx — keycap display (pure React take on shadcn Kbd, the standard shortcut
// display). Semantics use a real <kbd> element; keycap visuals use theme tokens
// (adaptive to light/dark), with a bottom shadow for depth.

import type { CSSProperties, ReactNode } from 'react';

export function Kbd({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return <kbd style={{ ...kbdStyle, ...style }}>{children}</kbd>;
}

/** Combo keys: a row of keycaps (shadcn KbdGroup), e.g. "Left ⌥", "Space". */
export function KbdGroup({ keys, style }: { keys: string[]; style?: CSSProperties }) {
  return (
    <span style={{ ...groupStyle, ...style }}>
      {keys.map((key, i) => (
        <Kbd key={`${key}-${i}`}>{key}</Kbd>
      ))}
    </span>
  );
}

const kbdStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  minWidth: 20,
  height: 21,
  padding: '0 6px',
  borderRadius: 5,
  fontSize: 11.5,
  lineHeight: 1,
  fontWeight: 500,
  fontFamily: 'var(--ol-font-sans)',
  color: 'var(--ol-ink-2)',
  background: 'var(--ol-surface-2)',
  border: '0.5px solid var(--ol-line-strong)',
  // Keycap depth: an extra shadow line at the bottom edge.
  boxShadow: '0 1.5px 0 var(--ol-line), 0 0 0 0.5px rgba(0,0,0,0.02)',
  whiteSpace: 'nowrap',
};

const groupStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 4,
};
