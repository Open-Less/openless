// useAppState.ts — minimal app-level state (current tab + settings modal).

import { useState } from 'react';

export type AppTab = 'overview' | 'history' | 'vocab' | 'style';

export interface AppState {
  currentTab: AppTab;
  setCurrentTab: (tab: AppTab) => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
}

export function useAppState(initialTab: AppTab = 'overview', initialSettings = false): AppState {
  const [currentTab, setCurrentTab] = useState<AppTab>(initialTab);
  const [settingsOpen, setSettingsOpen] = useState<boolean>(initialSettings);
  return { currentTab, setCurrentTab, settingsOpen, setSettingsOpen };
}
