import {
  applyStackedLayout,
  applyStackedLayoutFromPrefs,
  isStackedLayoutActive,
} from './stackedLayout';

function assert(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

const previousDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
const dataset: Record<string, string> = {};
Object.defineProperty(globalThis, 'document', {
  configurable: true,
  value: { documentElement: { dataset } },
});

try {
  applyStackedLayoutFromPrefs(true);
  assert(dataset.olStackedLayout === 'true', 'enabled preference should set the root layout attribute');
  assert(isStackedLayoutActive(), 'active query should reflect the root layout attribute');

  applyStackedLayoutFromPrefs(false);
  assert(!('olStackedLayout' in dataset), 'disabled preference should remove the root layout attribute');
  assert(!isStackedLayoutActive(), 'active query should become false after removal');

  applyStackedLayout(true);
  applyStackedLayoutFromPrefs(undefined);
  assert(!('olStackedLayout' in dataset), 'missing preference should restore the base layout');
} finally {
  if (previousDocument) {
    Object.defineProperty(globalThis, 'document', previousDocument);
  } else {
    delete (globalThis as { document?: unknown }).document;
  }
}

console.log('stacked layout tests passed');
