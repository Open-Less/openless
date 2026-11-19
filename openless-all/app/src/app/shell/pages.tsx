import { lazy, type ComponentType } from 'react';
import { History } from '../../features/history/History';
import { Overview } from '../../features/overview/Overview';
import type { AppTab } from './navigation';

const loaders = {
  usage: () => import('../../features/usage/Usage').then((module) => ({ default: module.Usage })),
  vocab: () =>
    import('../../features/vocabulary/Vocab').then((module) => ({ default: module.Vocab })),
  style: () =>
    import('../../features/style-packs/Style').then((module) => ({ default: module.Style })),
  marketplace: () =>
    import('../../features/marketplace/Marketplace').then((module) => ({
      default: module.Marketplace,
    })),
  translation: () =>
    import('../../features/translation/Translation').then((module) => ({
      default: module.Translation,
    })),
  selectionAsk: () =>
    import('../../features/selection-guide/SelectionAsk').then((module) => ({
      default: module.SelectionAsk,
    })),
  quickNote: () =>
    import('../../features/quick-note/QuickNote').then((module) => ({ default: module.QuickNote })),
  corrections: () =>
    import('../../features/corrections/Corrections').then((module) => ({
      default: module.Corrections,
    })),
};

export { Overview };
export const SelectionAsk = lazy(loaders.selectionAsk);

const pages: Partial<Record<AppTab, ComponentType>> = {
  // History is a primary destination on both desktop and mobile.
  history: History,
  usage: lazy(loaders.usage),
  vocab: lazy(loaders.vocab),
  style: lazy(loaders.style),
  marketplace: lazy(loaders.marketplace),
  translation: lazy(loaders.translation),
  selectionAsk: SelectionAsk,
  quickNote: lazy(loaders.quickNote),
  corrections: lazy(loaders.corrections),
};

export function getPage(tab: AppTab) {
  return pages[tab] ?? Overview;
}

export function preloadPage(tab: AppTab) {
  if (tab in loaders) void loaders[tab as keyof typeof loaders]().catch(() => {});
}
