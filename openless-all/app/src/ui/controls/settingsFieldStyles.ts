import type { CSSProperties } from 'react';

export const btnGhostStyle: CSSProperties = {
  padding: '5px 10px',
  fontSize: 12,
  borderRadius: 'var(--ol-r-sm)',
  border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-control-solid)',
  color: 'var(--ol-ink-2)',
  cursor: 'default',
  fontFamily: 'inherit',
  maxWidth: '100%',
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
};

export const inputStyle: CSSProperties = {
  flex: 1,
  height: 32,
  padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 'var(--ol-control-radius)',
  fontSize: 13.5,
  fontFamily: 'inherit',
  outline: 'none',
  // Same background as the SelectLite trigger: previously --ol-surface-2 (light gray) made every input /
  // dropdown inconsistent with other settings controls (mic / capsule style, etc., which use select-trigger-bg).
  background: 'var(--ol-select-trigger-bg)',
  width: '100%',
  maxWidth: 360,
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
};
