import { applyStackedLayoutFromPrefs } from './stackedLayout';
import { applyConservativeLayout } from './conservativeLayout';

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
  dataset.olStackedLayout = 'true';
  applyStackedLayoutFromPrefs(true);
  assert(
    !('olStackedLayout' in dataset),
    'retired enabled preference must clear the old root layout attribute',
  );

  applyStackedLayoutFromPrefs(false);
  assert(
    !('olStackedLayout' in dataset),
    'disabled preference should remove the root layout attribute',
  );
  dataset.olStackedLayout = 'true';
  applyStackedLayoutFromPrefs(undefined);
  assert(!('olStackedLayout' in dataset), 'missing preference should restore the base layout');
  dataset.olConservativeLayout = 'true';
  applyConservativeLayout(true);
  assert(!('olConservativeLayout' in dataset), 'retired single-column layout must not reactivate');
} finally {
  if (previousDocument) {
    Object.defineProperty(globalThis, 'document', previousDocument);
  } else {
    delete (globalThis as { document?: unknown }).document;
  }
}

console.log('stacked layout tests passed');
