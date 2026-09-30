import type { AppTab } from '../state/useAppState';

/** i18n key for a group's sub-item title: style → nav.polishMode, everything else nav.<id>. */
export function subItemLabelKey(id: AppTab): string {
  if (id === 'style') return 'nav.polishMode';
  return `nav.${id}`;
}
