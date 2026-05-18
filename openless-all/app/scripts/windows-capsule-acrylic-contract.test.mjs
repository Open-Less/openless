import { readFile } from 'node:fs/promises';

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

const libRs = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8');
const capsuleTsx = await readFile(new URL('../src/components/Capsule.tsx', import.meta.url), 'utf-8');
const capsuleLayoutTs = await readFile(new URL('../src/lib/capsuleLayout.ts', import.meta.url), 'utf-8');

assertMatch(
  libRs,
  /fn apply_windows_capsule_acrylic_region<R: Runtime>\([\s\S]*?SetWindowRgn\(hwnd,\s*region,\s*true\)/,
  'windows capsule should clip its native Acrylic to pill/badge regions instead of tinting the whole host',
);

assertMatch(
  libRs,
  /apply_windows_capsule_acrylic_region\(&capsule,\s*false\)[\s\S]*?apply_acrylic\(&capsule,\s*Some\(\(30,\s*32,\s*38,\s*140\)\)\)/,
  'windows capsule should keep Acrylic, but only after the native host region is clipped',
);

assertMatch(
  libRs,
  /position_capsule_bottom_center[\s\S]*?apply_windows_capsule_acrylic_region\(window,\s*translation_active\)/,
  'windows capsule should update the Acrylic region when translation mode changes the host height',
);

assertMatch(
  libRs,
  /CombineRgn\(region,\s*region,\s*badge_region,\s*RGN_OR\)/,
  'windows translation badge should be included as a separate rounded native region',
);

assertMatch(
  libRs,
  /apply_acrylic\(&qa,\s*Some\(\(30,\s*32,\s*38,\s*140\)\)\)/,
  'windows QA window may keep Acrylic because its panel fills the native host',
);

assertMatch(
  capsuleLayoutTs,
  /return \{ width: 196, height: 52, textWidth: 104, boxSizing: 'border-box' \};[\s\S]*?const horizontalInset = 12;[\s\S]*?width: pill\.width \+ horizontalInset \* 2,[\s\S]*?height: translationActive \? 118 : 84,[\s\S]*?bottomInset: 12,/,
  'windows capsule host must keep transparent margins for shadow, badge, and animation room',
);

assertMatch(
  capsuleTsx,
  /const useBackdrop = true;[\s\S]*?background: 'rgba\(255, 255, 255, 0\.85\)'/,
  'windows capsule should keep the original translucent pill surface because native Acrylic is clipped, not removed',
);

assertMatch(
  capsuleTsx,
  /return\s*\(\s*<div\s*style=\{\{[\s\S]*?width:\s*'100%',[\s\S]*?height:\s*'100%',[\s\S]*?paddingLeft:\s*hostMetrics\.horizontalInset,[\s\S]*?paddingRight:\s*hostMetrics\.horizontalInset,[\s\S]*?background:\s*'transparent'/,
  'capsule host should remain transparent outside the visible pill',
);
