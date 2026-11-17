import { lazy, type ComponentType } from 'react';
import { Overview } from '../../pages/Overview';
import type { AppTab } from './navigation';

const loaders = {
  history: () => import('../../pages/History').then((module) => ({ default: module.History })),
  usage: () => import('../../pages/Usage').then((module) => ({ default: module.Usage })),
  vocab: () => import('../../pages/Vocab').then((module) => ({ default: module.Vocab })),
  style: () => import('../../pages/Style').then((module) => ({ default: module.Style })),
  marketplace: () =>
    import('../../pages/Marketplace').then((module) => ({ default: module.Marketplace })),
  translation: () =>
    import('../../pages/Translation').then((module) => ({ default: module.Translation })),
  selectionAsk: () =>
    import('../../pages/SelectionAsk').then((module) => ({ default: module.SelectionAsk })),
  quickNote: () =>
    import('../../pages/QuickNote').then((module) => ({ default: module.QuickNote })),
  corrections: () =>
    import('../../pages/Corrections').then((module) => ({ default: module.Corrections })),
};

export { Overview };
export const SelectionAsk = lazy(loaders.selectionAsk);

const pages: Partial<Record<AppTab, ComponentType>> = {
  history: lazy(loaders.history),
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
