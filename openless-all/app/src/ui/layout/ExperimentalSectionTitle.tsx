import type { CSSProperties, ReactNode } from 'react';
import { SectionTitle } from './SectionTitle';
import { ExperimentalSectionLabel } from './ExperimentalSectionLabel';

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
