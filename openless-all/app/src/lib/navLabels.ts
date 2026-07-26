import type { AppTab } from '../state/useAppState';

/** 分组子项标题的 i18n key：style → nav.polishMode，其余 nav.<id>。 */
export function subItemLabelKey(id: AppTab): string {
  if (id === 'style') return 'nav.polishMode';
  return `nav.${id}`;
}
