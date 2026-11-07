import type { OS } from '../../components/WindowChrome';
import type { PlatformKind } from '../../lib/types';

export type SettingsSectionId =
  | 'general'
  | 'inputMethod'
  | 'shortcuts'
  | 'appearance'
  | 'services'
  | 'cloudServices'
  | 'privacy'
  | 'advanced'
  | 'about';

export const ADVANCED_PAGES = [
  { id: 'localModels', icon: 'mic', titleKey: 'experimentalAsr.title', beta: true },
  {
    id: 'voiceEnhancement',
    icon: 'mic',
    titleKey: 'settings.advancedPages.voiceEnhancementTitle',
    beta: true,
  },
  {
    id: 'cursorContext',
    icon: 'search',
    titleKey: 'settings.advancedPages.cursorContextTitle',
    beta: true,
  },
  {
    id: 'startupDiagnostics',
    icon: 'bolt',
    titleKey: 'settings.advancedPages.startupDiagnosticsTitle',
  },
  {
    id: 'vocabularyLearning',
    icon: 'pencil',
    titleKey: 'settings.vocabularyLearning.title',
    beta: true,
  },
  { id: 'customSyncServer', icon: 'cloud', titleKey: 'cloudSyncE2ee.customServerTitle' },
  { id: 'debug', icon: 'bolt', titleKey: 'settings.debug.title' },
] as const;

export type AdvancedPage = (typeof ADVANCED_PAGES)[number];
export type AdvancedPageId = AdvancedPage['id'];

export function visibleAdvancedPages(platform: PlatformKind | undefined, os: OS): AdvancedPage[] {
  return ADVANCED_PAGES.filter((page) => {
    if (page.id === 'localModels') return platform === 'desktop' && os === 'mac';
    if (page.id === 'vocabularyLearning') return platform === 'desktop' || platform === 'android';
    if (page.id === 'debug')
      return platform === 'desktop' || platform === 'android' || platform === 'harmony';
    return true;
  });
}

// Native readers exist on macOS, Windows, and Android. Generic mobile and Harmony
// have no cursor-context capture, so the toggle stays hidden there.
export function showsCursorContextControl(platform: PlatformKind | undefined, os: OS): boolean {
  if (platform === 'mobile' || platform === 'harmony' || os === 'harmony') return false;
  if (platform === 'android' || os === 'android') return true;
  return os === 'mac' || os === 'win';
}

export interface SettingsNavigationItem {
  id: SettingsSectionId;
  icon: string;
}

export const SETTINGS_SECTIONS: SettingsNavigationItem[] = [
  { id: 'general', icon: 'mic' },
  { id: 'inputMethod', icon: 'keyboard' },
  { id: 'shortcuts', icon: 'bolt' },
  { id: 'services', icon: 'sparkle' },
  { id: 'cloudServices', icon: 'cloud' },
  { id: 'appearance', icon: 'settings' },
  { id: 'privacy', icon: 'shield' },
  { id: 'advanced', icon: 'flask' },
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

export function availableServiceViews(localModels: boolean, os?: OS): ServiceViewId[] {
  return [
    'llm',
    'asr',
    'omni',
    ...(localModels || os === 'mac' ? (['models'] as const) : []),
    'connections',
  ];
}

export function isServiceViewInactive(
  view: ServiceViewId,
  multimodal: boolean,
  os?: OS,
  voiceActivationModels = false,
): boolean {
  if (view === 'models' && os === 'win' && voiceActivationModels) return false;
  if (view === 'models' && os === 'mac') return true;
  return multimodal ? view !== 'omni' && view !== 'connections' : view === 'omni';
}

export function resolveServiceView(
  requested: ServiceViewId,
  available: ServiceViewId[],
  multimodal: boolean,
  os?: OS,
  voiceActivationModels = false,
): ServiceViewId {
  const enabled = available.filter(
    (view) => !isServiceViewInactive(view, multimodal, os, voiceActivationModels),
  );
  return enabled.includes(requested) ? requested : enabled[0];
}
