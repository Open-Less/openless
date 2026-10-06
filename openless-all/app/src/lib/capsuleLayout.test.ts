import {
  getCapsuleHostMetrics,
  getCapsuleMessageLayout,
  getCapsulePillMetrics,
  getCapsuleTranscriptRailHeight,
  getCapsuleTranscriptRailPosition,
  getClassicCapsuleGeometry,
  getClassicProcessingLabel,
  getClassicTranslationBadgeOffset,
  parseCapsuleStyle,
} from './capsuleLayout.ts';

function assertEqual<T>(actual: T, expected: T, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

function assertClose(actual: number, expected: number, name: string) {
  if (Math.abs(actual - expected) > 1e-9) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

const winMetrics = getCapsulePillMetrics('win');
assertEqual(winMetrics.width, 460, 'windows voice orb stage uses the demo-scale width');
assertEqual(winMetrics.height, 180, 'windows voice orb stage uses the demo-scale height');
assertEqual(winMetrics.textWidth, 400, 'windows voice orb text stays inside the stage');
assertEqual(
  winMetrics.boxSizing,
  'border-box',
  'windows voice orb stage width is an outer border-box metric',
);

const winHost = getCapsuleHostMetrics('win', false);
assertEqual(winHost.width, 460, 'windows voice orb host matches stage width');
assertEqual(
  winHost.height,
  228,
  'windows voice orb host reserves the transcript rail above the stage',
);
assertEqual(winHost.horizontalInset, 0, 'windows voice orb host has no side button inset');
assertEqual(winHost.boxSizing, 'border-box', 'windows voice orb host keeps border-box sizing');
assertEqual(
  winHost.width - winHost.horizontalInset * 2,
  winMetrics.width,
  'windows voice orb host keeps the visible stage width after reserving side insets',
);
assertEqual(
  winHost.height - 48 - winHost.bottomInset,
  winMetrics.height,
  'windows voice orb host keeps the visible stage height below the transcript rail',
);

const winHostWithTranslation = getCapsuleHostMetrics('win', true);
assertEqual(
  winHostWithTranslation.width,
  460,
  'windows translation voice orb keeps the same outer width',
);
assertEqual(
  winHostWithTranslation.height,
  228,
  'windows translation voice orb keeps the transcript rail height',
);
assertEqual(
  winHostWithTranslation.horizontalInset,
  0,
  'windows translation voice orb has no side button inset',
);
assertEqual(
  winHostWithTranslation.boxSizing,
  'border-box',
  'windows translation host keeps border-box sizing',
);

const macMetrics = getCapsulePillMetrics('mac');
assertEqual(macMetrics.width, 460, 'mac voice orb stage uses the demo-scale width');
assertEqual(macMetrics.height, 180, 'mac voice orb stage uses the demo-scale height');
assertEqual(macMetrics.textWidth, 400, 'mac voice orb text stays inside the stage');
assertEqual(macMetrics.boxSizing, 'border-box', 'mac voice orb stage keeps border-box sizing');

const macHost = getCapsuleHostMetrics('mac', false);
assertEqual(macHost.width, 460, 'mac voice orb host matches stage width');
assertEqual(macHost.height, 228, 'mac voice orb host reserves the transcript rail above the stage');
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

for (const os of ['mac', 'win'] as const) {
  const classic = getCapsuleHostMetrics(os, false, 'classic');
  const typeless = getCapsuleHostMetrics(os, true, 'typeless');
  assertEqual(
    classic.height,
    172,
    `${os}: classic reserves the transcript rail and translating badge`,
  );
  assertEqual(typeless.height, 65, `${os}: typeless translation host reserves a badge row`);
  assertEqual(typeless.width, 206, `${os}: typeless window keeps the 1/5 stage width`);
  assertEqual(typeless.bottomInset, 0, `${os}: typeless pill hugs the work-area bottom edge`);
  assertEqual(
    getCapsuleHostMetrics(os, false, 'typeless').height,
    57,
    `${os}: typeless host stays compact when translation is hidden`,
  );
}
for (const style of ['siri', 'classic', 'typeless'] as const) {
  assertEqual(parseCapsuleStyle(style), style, `${style} is accepted from preferences and events`);
}
assertEqual(getCapsuleTranscriptRailHeight('siri'), 40, 'Siri rail height matches native overlay');
assertEqual(getCapsuleTranscriptRailHeight('classic'), 52, 'Classic rail height matches native overlay');
assertEqual(getCapsuleTranscriptRailHeight('typeless'), 52, 'Typeless logical rail height matches CSS overlay');
assertEqual(
  getClassicTranslationBadgeOffset(true),
  60,
  'Classic translation badge clears the external 52px rail and its 8px gap',
);
assertEqual(
  getClassicTranslationBadgeOffset(false),
  0,
  'Classic translation badge does not reserve a hidden rail',
);
assertEqual(
  getClassicTranslationBadgeOffset(true, false),
  0,
  'Classic external rail uses its own badge lane instead of same-window offset',
);
const classicExternalGeometry = getClassicCapsuleGeometry('win', true, true, false);
assertEqual(classicExternalGeometry.hostHeight, 172, 'Classic external rail uses the 172px host');
assertEqual(classicExternalGeometry.bodyTop, 104, 'Classic body is anchored at the bottom of the host');
assertEqual(classicExternalGeometry.railTop, 14, 'Classic external rail clears the translation badge lane');
assertEqual(classicExternalGeometry.badgeTop, 74, 'Classic external badge sits above the pill');
assertEqual(classicExternalGeometry.badgeBottom, 96, 'Classic external badge leaves an 8px body gap');
if (
  classicExternalGeometry.railTop! + classicExternalGeometry.railHeight > classicExternalGeometry.badgeTop ||
  classicExternalGeometry.badgeBottom > classicExternalGeometry.bodyTop
) {
  throw new Error('Classic external rail and translation badge must not overlap');
}
const classicSameWindowGeometry = getClassicCapsuleGeometry('win', true, true, true);
assertEqual(classicSameWindowGeometry.railTop, 44, 'Classic same-window rail keeps the body rail anchor');
assertEqual(classicSameWindowGeometry.badgeBottom, 36, 'Classic same-window badge clears the rail');
for (const os of ['mac', 'win'] as const) {
  const external = getClassicCapsuleGeometry(os, true, true, false);
  if (external.railTop! + external.railHeight > external.badgeTop || external.badgeBottom > external.bodyTop) {
    throw new Error(`${os}: external Classic rail and badge overlap`);
  }
}
assertEqual(
  getClassicProcessingLabel(false),
  'capsule.thinking',
  'Classic processing uses thinking copy for ordinary dictation',
);
assertEqual(
  getClassicProcessingLabel(true),
  'capsule.using',
  'Classic processing uses using copy for operating sessions',
);
assertEqual(
  getCapsuleTranscriptRailPosition('siri', false, true)?.topOffset,
  -48,
  'Siri rail sits above the 180px body with its 8px CSS gap',
);
assertEqual(
  getCapsuleTranscriptRailPosition('siri', true, true)?.topOffset,
  -48,
  'Siri translation does not move the full-height body rail anchor',
);
assertEqual(
  getCapsuleTranscriptRailPosition('classic', false, true)?.topOffset,
  44,
  'Classic compact external rail anchors to the 172px body content top',
);
assertEqual(
  getCapsuleTranscriptRailPosition('classic', true, true)?.topOffset,
  14,
  'Classic translation external rail clears the explicit badge lane',
);
const typelessCompactRail = getCapsuleTranscriptRailPosition('typeless', false, true);
const typelessTranslationRail = getCapsuleTranscriptRailPosition('typeless', true, true);
if (!typelessCompactRail || !typelessTranslationRail) throw new Error('typeless rail geometry missing');
assertClose(typelessCompactRail.height, 52 * 0.447, 'Typeless rail uses the zoomed native height');
assertClose(typelessCompactRail.topOffset, 57 - 64 * 0.447 - 52 * 0.447, 'Typeless compact body anchor');
assertClose(
  typelessTranslationRail.topOffset,
  65 - 20 * 0.447 - 64 * 0.447 - 52 * 0.447,
  'Typeless translation body anchor includes the translation row',
);
for (const style of ['siri', 'classic', 'typeless'] as const) {
  assertEqual(
    getCapsuleTranscriptRailPosition(style, false, false),
    null,
    `${style} hides the rail when transcript is not visible`,
  );
}
assertEqual(
  parseCapsuleStyle('unknown'),
  undefined,
  'unknown styles do not replace the active choice',
);
