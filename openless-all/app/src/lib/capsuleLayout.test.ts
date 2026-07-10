import {
  getCapsuleHostMetrics,
  getCapsuleMessageLayout,
  getCapsulePillMetrics,
} from './capsuleLayout.ts';

function assertEqual<T>(actual: T, expected: T, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

const winMetrics = getCapsulePillMetrics('win');
assertEqual(winMetrics.width, 460, 'windows voice orb stage uses the demo-scale width');
assertEqual(winMetrics.height, 180, 'windows voice orb stage uses the demo-scale height');
assertEqual(winMetrics.textWidth, 400, 'windows voice orb text stays inside the stage');
assertEqual(winMetrics.boxSizing, 'border-box', 'windows voice orb stage width is an outer border-box metric');

const winHost = getCapsuleHostMetrics('win', false);
assertEqual(winHost.width, 460, 'windows voice orb host matches stage width');
assertEqual(winHost.height, 180, 'windows voice orb host matches stage height');
assertEqual(winHost.horizontalInset, 0, 'windows voice orb host has no side button inset');
assertEqual(winHost.boxSizing, 'border-box', 'windows voice orb host keeps border-box sizing');
assertEqual(
  winHost.width - winHost.horizontalInset * 2,
  winMetrics.width,
  'windows voice orb host keeps the visible stage width after reserving side insets',
);
assertEqual(
  winHost.height - winHost.bottomInset,
  winMetrics.height,
  'windows voice orb host keeps the visible stage height after reserving bottom inset',
);

const winHostWithTranslation = getCapsuleHostMetrics('win', true);
assertEqual(winHostWithTranslation.width, 460, 'windows translation voice orb keeps the same outer width');
assertEqual(winHostWithTranslation.height, 180, 'windows translation voice orb keeps the same outer height');
assertEqual(winHostWithTranslation.horizontalInset, 0, 'windows translation voice orb has no side button inset');
assertEqual(winHostWithTranslation.boxSizing, 'border-box', 'windows translation host keeps border-box sizing');

const macMetrics = getCapsulePillMetrics('mac');
assertEqual(macMetrics.width, 460, 'mac voice orb stage uses the demo-scale width');
assertEqual(macMetrics.height, 180, 'mac voice orb stage uses the demo-scale height');
assertEqual(macMetrics.textWidth, 400, 'mac voice orb text stays inside the stage');
assertEqual(macMetrics.boxSizing, 'border-box', 'mac voice orb stage keeps border-box sizing');

const macHost = getCapsuleHostMetrics('mac', false);
assertEqual(macHost.width, 460, 'mac voice orb host matches stage width');
assertEqual(macHost.height, 180, 'mac voice orb host matches stage height');
assertEqual(macHost.boxSizing, 'border-box', 'mac voice orb host keeps border-box sizing');

const winErrorLayout = getCapsuleMessageLayout('win', 'error');
assertEqual(winErrorLayout.lineClamp, 2, 'windows error message allows two lines');
assertEqual(winErrorLayout.allowWrap, true, 'windows error message wraps');

const winProcessingLayout = getCapsuleMessageLayout('win', 'processing');
assertEqual(winProcessingLayout.lineClamp, 2, 'windows processing label allows two lines');
assertEqual(winProcessingLayout.allowWrap, true, 'windows processing label wraps');

const macErrorLayout = getCapsuleMessageLayout('mac', 'error');
assertEqual(macErrorLayout.lineClamp, 1, 'mac error message stays single-line');
assertEqual(macErrorLayout.allowWrap, false, 'mac error message stays nowrap');
