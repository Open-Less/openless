import { useEffect, useState } from 'react';
import { getShortcutPlatform } from '../../runtime/environment';

export type AppTab =
  | 'overview'
  | 'history'
  | 'usage'
  | 'vocab'
  | 'style'
  | 'appStyles'
  | 'marketplace'
  | 'translation'
  | 'quickNote'
  | 'corrections'
  | 'localAsr';

export function isTabAvailable(tab: AppTab): boolean {
  if (tab === 'appStyles') {
    const platform = getShortcutPlatform();
    return platform.isMac || platform.isWindows;
  }
  return true;
}

export interface AppState {
  currentTab: AppTab;
  setCurrentTab: (tab: AppTab) => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
}

export function useAppState(initialTab: AppTab = 'overview', initialSettings = false): AppState {
  const [currentTab, setCurrentTab] = useState<AppTab>(() =>
    isTabAvailable(initialTab) ? initialTab : 'overview',
  );
  const [settingsOpen, setSettingsOpen] = useState<boolean>(initialSettings);
  return {
    currentTab,
    setCurrentTab: (tab) => {
      if (isTabAvailable(tab)) setCurrentTab(tab);
    },
    settingsOpen,
    setSettingsOpen,
  };
}

/** i18n key for a group's sub-item title: style → nav.polishMode, everything else nav.<id>. */
export function subItemLabelKey(id: AppTab): string {
  if (id === 'style') return 'nav.polishMode';
  if (id === 'appStyles') return 'appStyles.title';
  return `nav.${id}`;
}

interface NavItem {
  id: AppTab;
  icon: string;
}

export const MORE_TABS: NavItem[] = [
  { id: 'vocab', icon: 'vocab' },
  { id: 'translation', icon: 'translate' },
  { id: 'quickNote', icon: 'history' },
  { id: 'corrections', icon: 'filter' },
];

export const STYLE_TABS: NavItem[] = [
  { id: 'style', icon: 'style' },
  { id: 'marketplace', icon: 'sparkle' },
];

export const MOBILE_BOTTOM_TABS: NavItem[] = [
  { id: 'overview', icon: 'overview' },
  { id: 'history', icon: 'history' },
];

type NavNode =
  | { kind: 'item'; id: AppTab; icon: string }
  | { kind: 'group'; key: string; icon: string; children: Array<{ id: AppTab }> };

export const NAV_TREE: NavNode[] = [
  { kind: 'item', id: 'overview', icon: 'overview' },
  { kind: 'item', id: 'history', icon: 'history' },
  { kind: 'item', id: 'vocab', icon: 'vocab' },
  {
    kind: 'group',
    key: 'style',
    icon: 'style',
    children: [{ id: 'style' }, { id: 'marketplace' }, { id: 'appStyles' }],
  },
  {
    kind: 'group',
    key: 'tools',
    icon: 'selectionAsk',
    children: [
      { id: 'corrections' },
      { id: 'translation' },
      { id: 'quickNote' },
    ],
  },
];

function groupOfTab(tab: AppTab): string | undefined {
  for (const node of NAV_TREE) {
    if (node.kind === 'group' && node.children.some((child) => child.id === tab)) return node.key;
  }
  return undefined;
}

export function useNavigation(initialTab: AppTab, initialSettings: boolean) {
  const state = useAppState(initialTab, initialSettings);
  const group = groupOfTab(state.currentTab);
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>(() =>
    group ? { [group]: true } : {},
  );
  useEffect(() => {
    if (group) setOpenGroups((open) => (open[group] ? open : { ...open, [group]: true }));
  }, [state.currentTab, group]);
  const toggleGroup = (key: string) => setOpenGroups((open) => ({ ...open, [key]: !open[key] }));
  return { ...state, openGroups, toggleGroup };
}
