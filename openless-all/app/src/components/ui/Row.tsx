import type { ReactNode } from 'react';
import { SettingRow } from '../../ui/controls/SettingRow';

export function Row({
  label,
  desc,
  children,
}: {
  label: string;
  desc?: string;
  children: ReactNode;
}) {
  return (
    <SettingRow label={label} desc={desc} descriptionPlacement="inline">
      {children}
    </SettingRow>
  );
}
