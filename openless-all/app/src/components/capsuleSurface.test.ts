// The recording pill's top edge must read as a soft sheen, not a 1px hard line.
//
// Regression guard: `.ol-default-capsule::before` used to carry
// `box-shadow: inset 0 1px 0 var(--ol-pill-highlight)` with an 82%-opaque white
// highlight. At 1x that renders as a single solid row of pixels, which the eye
// reads as a drawn line sitting on top of the capsule instead of light falling
// across a rounded surface. The fix is a multi-stop vertical gradient.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

function readCss(name: string) {
  return readFileSync(join(here, name), 'utf8');
}

function assertTrue(condition: boolean, name: string) {
  if (!condition) throw new Error(`FAIL: ${name}`);
}

const capsuleCss = readCss('OpenLessCapsule.css');
const stylesCss = readCss('CapsuleStyles.css');
const allCss = `${capsuleCss}\n${stylesCss}`;

// 1. No opaque 1px inset line on the pill surface.
assertTrue(
  !/inset\s+0\s+1px\s+0\s+var\(--ol-pill-highlight\)/.test(allCss),
  'pill surface must not use `inset 0 1px 0 var(--ol-pill-highlight)` (hard 1px line)',
);

// 2. The sheen token exists and is a gradient, not a flat colour.
assertTrue(
  /--ol-pill-sheen:\s*linear-gradient\(/.test(allCss),
  '`--ol-pill-sheen` must be defined as a linear-gradient',
);

// 3. The gradient needs at least three stops so the ramp is continuous.
const sheenBlocks = allCss.match(/--ol-pill-sheen:\s*linear-gradient\([^;]*\);/g) ?? [];
assertTrue(sheenBlocks.length >= 2, `expected light + dark sheen tokens, found ${sheenBlocks.length}`);
for (const block of sheenBlocks) {
  const stops = (block.match(/(rgb|rgba|#|transparent)/g) ?? []).length;
  assertTrue(stops >= 3, `sheen gradient needs >=3 colour stops for a smooth ramp: ${block}`);
}

// 4. Both themes must override it, so dark mode does not fall back to a hard edge.
assertTrue(
  /\[data-ol-theme='dark'\][\s\S]{0,400}?--ol-pill-sheen:/.test(stylesCss),
  'dark theme must define its own `--ol-pill-sheen`',
);

// 5. The surface must actually consume the sheen.
assertTrue(
  /\.ol-default-capsule::before\s*\{[\s\S]*?background:[\s\S]*?var\(--ol-pill-sheen\)/.test(capsuleCss) ||
    /\.ol-default-capsule::before\s*\{[\s\S]*?box-shadow:[\s\S]*?var\(--ol-pill-sheen\)/.test(capsuleCss),
  '`.ol-default-capsule::before` must paint `var(--ol-pill-sheen)`',
);

// 6. The translation badge shared the same hard line; it must not keep one either.
assertTrue(
  !/inset\s+0\s+1px\s+0\s+var\(--ol-pill-highlight\)/.test(stylesCss),
  'translation badge must not keep the hard 1px top line',
);

console.log('capsuleSurface: ok');
