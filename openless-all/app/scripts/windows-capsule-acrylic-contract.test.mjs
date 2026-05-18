import { readFile } from 'node:fs/promises';

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

function assertNotMatch(source, pattern, name) {
  if (pattern.test(source)) {
    throw new Error(`${name}: forbidden pattern ${pattern} found`);
  }
}

const libRs = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8');
const capsuleTsx = await readFile(new URL('../src/components/Capsule.tsx', import.meta.url), 'utf-8');
const capsuleLayoutTs = await readFile(new URL('../src/lib/capsuleLayout.ts', import.meta.url), 'utf-8');

assertMatch(
  libRs,
  /fn apply_windows_capsule_material_region<R: Runtime>\([\s\S]*?DwmEnableBlurBehindWindow\(hwnd,\s*&blur\)[\s\S]*?SetWindowRgn\(hwnd,\s*paint_region,\s*true\)/,
  'windows capsule should use a DWM blur region and a native paint region instead of tinting the whole host',
);

assertMatch(
  libRs,
  /DwmSetWindowAttribute\([\s\S]*?DWMWA_SYSTEMBACKDROP_TYPE[\s\S]*?DWMSBT_NONE[\s\S]*?DWM_BB_ENABLE \| DWM_BB_BLURREGION/,
  'windows capsule should explicitly disable full-window Win11 system backdrop before enabling region-scoped native blur',
);

assertNotMatch(
  libRs,
  /apply_acrylic\(&capsule,/,
  'windows capsule must not use window-vibrancy Acrylic because it paints a rectangular grey host on Win11',
);

assertMatch(
  libRs,
  /position_capsule_bottom_center[\s\S]*?apply_windows_capsule_material_region\(window,\s*translation_active\)/,
  'windows capsule should update the material region when translation mode changes the host height',
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
  'windows capsule should keep the original translucent pill surface because native material is region-scoped, not removed',
);

assertMatch(
  capsuleTsx,
  /return\s*\(\s*<div\s*style=\{\{[\s\S]*?width:\s*'100%',[\s\S]*?height:\s*'100%',[\s\S]*?paddingLeft:\s*hostMetrics\.horizontalInset,[\s\S]*?paddingRight:\s*hostMetrics\.horizontalInset,[\s\S]*?background:\s*'transparent'/,
  'capsule host should remain transparent outside the visible pill',
);
