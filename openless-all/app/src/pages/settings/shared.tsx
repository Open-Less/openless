// Setting headings, legacy input styles and display-only provider labels.
import type { CSSProperties, ReactNode } from 'react';
import { ExperimentalSectionLabel } from '../../features/local-asr/ExperimentalSectionLabel';
import { Hint } from '../../ui/overlays/Tooltip';

export function SectionTitle({
  children,
  hint,
  style,
}: {
  children: ReactNode;
  /** Functional explanation shown on hovering the title text, for sections whose purpose isn't obvious from the name (e.g. Less Computer). */
  hint?: string;
  style?: CSSProperties;
}) {
  const titleStyle: CSSProperties = {
    fontSize: 14,
    fontWeight: 'var(--ol-font-weight-semibold)',
    color: 'var(--ol-ink)',
    marginBottom: 6,
    letterSpacing: '-0.01em',
    ...style,
  };
  if (!hint) {
    return <div style={titleStyle}>{children}</div>;
  }
  return (
    <div style={titleStyle}>
      <Hint content={hint}>{children}</Hint>
    </div>
  );
}

export function ExperimentalSectionTitle({
  children,
  badge,
  hint,
  style,
}: {
  children: ReactNode;
  badge: string;
  hint?: string;
  style?: CSSProperties;
}) {
  return (
    <SectionTitle hint={hint} style={style}>
      <ExperimentalSectionLabel badge={badge}>{children}</ExperimentalSectionLabel>
    </SectionTitle>
  );
}

// Page slimming: settings page description copy is hidden entirely (component signature + call sites kept for easy restoration).
export function SectionDesc(_props: { children: ReactNode; style?: CSSProperties }) {
  return null;
}

export { SettingRow, Toggle } from '../../ui/controls/SettingRow';

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

export { ASR_LABELS } from '../../features/providers/labels';
