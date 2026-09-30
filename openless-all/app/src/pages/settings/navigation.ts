import type { OS } from '../../components/WindowChrome';
import type { PlatformKind } from '../../lib/types';

export type SettingsSectionId =
  | 'general'
  | 'inputMethod'
  | 'shortcuts'
  | 'appearance'
  | 'services'
  | 'privacy'
  | 'advanced'
  | 'about';

export const ADVANCED_PAGES = [
  { id: 'lessComputer', icon: 'mac', titleKey: 'settings.codingAgent.title' },
  { id: 'claudeConsole', icon: 'chevLR', titleKey: 'settings.codingConsole.title' },
  { id: 'vocabularyLearning', icon: 'sparkle', titleKey: 'settings.vocabularyLearning.title' },
  { id: 'debug', icon: 'bolt', titleKey: 'settings.debug.title' },
] as const;

export type AdvancedPage = (typeof ADVANCED_PAGES)[number];
export type AdvancedPageId = AdvancedPage['id'];

export function visibleAdvancedPages(platform: PlatformKind | undefined, os: OS): AdvancedPage[] {
  return ADVANCED_PAGES.filter((page) => {
    if (page.id === 'lessComputer') return platform === 'desktop' && (os === 'mac' || os === 'win');
    if (page.id === 'claudeConsole') return platform === 'desktop' && os === 'mac';
    if (page.id === 'vocabularyLearning')
      return platform === 'android' || (platform === 'desktop' && (os === 'win' || os === 'mac'));
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
  { id: 'inputMethod', icon: 'keyboard' },
  { id: 'shortcuts', icon: 'bolt' },
  { id: 'services', icon: 'cloud' },
  { id: 'appearance', icon: 'settings' },
  { id: 'privacy', icon: 'shield' },
  { id: 'advanced', icon: 'sparkle' },
  { id: 'about', icon: 'info' },
];

export function visibleSettingsSections(
  supportsDesktopHotkey: boolean,
  platform?: PlatformKind,
): SettingsNavigationItem[] {
  return SETTINGS_SECTIONS.filter((item) => {
    if (item.id === 'shortcuts') return supportsDesktopHotkey;
    if (item.id === 'inputMethod') return platform === 'android';
    return true;
  });
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

export function availableServiceViews(localModels: boolean): ServiceViewId[] {
  return ['llm', 'asr', 'omni', ...(localModels ? (['models'] as const) : []), 'connections'];
}

export function isServiceViewInactive(view: ServiceViewId, multimodal: boolean): boolean {
  return multimodal ? view !== 'omni' && view !== 'connections' : view === 'omni';
}

export function resolveServiceView(
  requested: ServiceViewId,
  available: ServiceViewId[],
  multimodal: boolean,
): ServiceViewId {
  const enabled = available.filter((view) => !isServiceViewInactive(view, multimodal));
  return enabled.includes(requested) ? requested : enabled[0];
}
