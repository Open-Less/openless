import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(new URL('..', import.meta.url).pathname);
const floatingShell = readFileSync(resolve(root, 'src/components/FloatingShell.tsx'), 'utf8');
const transitionViewport = readFileSync(
  resolve(root, 'src/components/ui/TransitionViewport.tsx'),
  'utf8',
);
const capsuleStyles = readFileSync(resolve(root, 'src/components/CapsuleStyles.css'), 'utf8');
const insertMotion = readFileSync(resolve(root, 'src/lib/insertTextAnimation.ts'), 'utf8');

assert.match(
  floatingShell,
  /import \{ SettingsModal, type SettingsSectionId \} from '\.\/SettingsModal';/,
  'settings must be ready before the first open instead of lazy-loading a flash placeholder',
);
assert.doesNotMatch(
  floatingShell,
  /const SettingsModal = lazy\(/,
  'settings must not be a first-open lazy chunk',
);
assert.match(
  transitionViewport,
  /className="ol-transition-mask"/,
  'page transitions must render a pointer-transparent cover during handoff',
);
assert.match(
  capsuleStyles,
  /\.ol-live-transcript-pill[\s\S]*?clip-path: inset\(0 round 999px\)/,
  'streaming transcript pill must clip its full rounded shell',
);
assert.match(
  capsuleStyles,
  /\.ol-live-transcript-track\[data-overflow='true'\][\s\S]*?-webkit-mask-image/,
  'streaming transcript overflow fade must be clipped consistently in WebKit',
);
assert.match(insertMotion, /bornBlurPx: 4\.5/);
assert.match(insertMotion, /bornFadeMs: 220/);
console.log('UI follow-up motion, settings flash and transcript radius contracts passed');
