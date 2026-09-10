import type { OS } from '../../components/WindowChrome';
import type { PlatformKind } from '../../lib/types';

export type SettingsSectionId =
  'general' | 'shortcuts' | 'appearance' | 'services' | 'privacy' | 'advanced' | 'about';

export const ADVANCED_PAGES = [
  { id: 'lessComputer', icon: 'mac', titleKey: 'settings.codingAgent.title' },
  { id: 'claudeConsole', icon: 'chevLR', titleKey: 'settings.codingConsole.title' },
  { id: 'multimodal', icon: 'sparkle', titleKey: 'settings.advanced.multimodalPipelineTitle' },
  { id: 'debug', icon: 'bolt', titleKey: 'settings.debug.title' },
] as const;

export type AdvancedPage = (typeof ADVANCED_PAGES)[number];
export type AdvancedPageId = AdvancedPage['id'];

export function visibleAdvancedPages(platform: PlatformKind | undefined, os: OS): AdvancedPage[] {
  return ADVANCED_PAGES.filter((page) => {
    if (page.id === 'lessComputer') return platform === 'desktop' && (os === 'mac' || os === 'win');
    if (page.id === 'claudeConsole') return platform === 'desktop' && os === 'mac';
    if (page.id === 'debug') return platform === 'desktop' || platform === 'android';
    return true;
  });
}

export interface SettingsNavigationItem {
  id: SettingsSectionId;
  icon: string;
}

export const SETTINGS_SECTIONS: SettingsNavigationItem[] = [
  { id: 'general', icon: 'mic' },
  { id: 'shortcuts', icon: 'bolt' },
  { id: 'services', icon: 'cloud' },
  { id: 'appearance', icon: 'settings' },
  { id: 'privacy', icon: 'shield' },
  { id: 'advanced', icon: 'sparkle' },
  { id: 'about', icon: 'info' },
];

export function visibleSettingsSections(supportsDesktopHotkey: boolean): SettingsNavigationItem[] {
  return SETTINGS_SECTIONS.filter((item) => item.id !== 'shortcuts' || supportsDesktopHotkey);
}

export interface SearchableSettingsSection extends SettingsNavigationItem {
  title: string;
  description: string;
  keywords: string;
}

export function searchSettingsSections<T extends SearchableSettingsSection>(
  sections: T[],
  query: string,
): T[] {
  const normalize = (text: string) => text.normalize('NFKC').toLocaleLowerCase();
  const terms = normalize(query).trim().split(/\s+/).filter(Boolean);
  return sections.filter((item) => {
    const text = normalize(`${item.title} ${item.description} ${item.keywords}`);
    return terms.every((term) => text.includes(term));
  });
}

export type ServiceViewId = 'llm' | 'asr' | 'omni' | 'models' | 'connections';

export function availableServiceViews(multimodal: boolean, localModels: boolean): ServiceViewId[] {
  return [
    ...(multimodal ? ['omni' as const] : ['llm' as const, 'asr' as const]),
    ...(localModels ? ['models' as const] : []),
    'connections',
  ];
}

export function resolveServiceView(
  requested: ServiceViewId,
  available: ServiceViewId[],
): ServiceViewId {
  return available.includes(requested) ? requested : available[0];
}
