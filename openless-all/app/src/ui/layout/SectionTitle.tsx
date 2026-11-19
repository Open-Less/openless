import type { CSSProperties, ReactNode } from 'react';
import { Hint } from '../overlays/Tooltip';

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
