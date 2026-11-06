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
  { id: 'localModels', icon: 'mic', titleKey: 'experimentalAsr.title' },
  {
    id: 'voiceEnhancement',
    icon: 'mic',
    titleKey: 'settings.advancedPages.voiceEnhancementTitle',
    group: 'recording',
  },
  {
    id: 'cursorContext',
    icon: 'search',
    titleKey: 'settings.advancedPages.cursorContextTitle',
    group: 'text',
    inline: true,
  },
  {
    id: 'startupDiagnostics',
    icon: 'bolt',
    titleKey: 'settings.advancedPages.startupDiagnosticsTitle',
    group: 'diagnostics',
    inline: true,
  },
  { id: 'testingFeatures', icon: 'sparkle', titleKey: 'settings.testingFeatures.title' },
  { id: 'lessComputer', icon: 'mac', titleKey: 'settings.codingAgent.title' },
  { id: 'claudeConsole', icon: 'chevLR', titleKey: 'settings.codingConsole.title' },
  { id: 'customSyncServer', icon: 'cloud', titleKey: 'cloudSyncE2ee.customServerTitle' },
  { id: 'debug', icon: 'bolt', titleKey: 'settings.debug.title' },
] as const;

export type AdvancedPage = (typeof ADVANCED_PAGES)[number];
export type AdvancedPageId = AdvancedPage['id'];

export function isTestingFeaturePage(page: AdvancedPageId | null): boolean {
  return page === 'lessComputer' || page === 'claudeConsole';
}

export function visibleAdvancedPages(
  platform: PlatformKind | undefined,
  os: OS,
  enabled = false,
): AdvancedPage[] {
  return ADVANCED_PAGES.filter((page) => {
    if (page.id === 'localModels') return platform === 'desktop' && os === 'mac';
    if (isTestingFeaturePage(page.id) && !enabled) return false;
    if (page.id === 'lessComputer') return platform === 'desktop' && (os === 'mac' || os === 'win');
    if (page.id === 'claudeConsole') return platform === 'desktop' && os === 'mac';
    if (page.id === 'debug')
      return platform === 'desktop' || platform === 'android' || platform === 'harmony';
    return true;
  });
}

export type AdvancedPageGroup = 'recording' | 'text' | 'diagnostics';

export function advancedPageGroup(page: AdvancedPage): AdvancedPageGroup | undefined {
  return 'group' in page ? page.group : undefined;
}

// Cursor context and startup diagnostics stay on the advanced root.
// Voice enhancement opens its own page, then a third screen for the DSP sliders.
export function isInlineAdvancedPage(page: AdvancedPage): boolean {
  return 'inline' in page && page.inline;
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
