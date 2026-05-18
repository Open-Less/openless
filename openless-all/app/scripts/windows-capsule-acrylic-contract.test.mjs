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

assertNotMatch(
  libRs,
  /apply_acrylic\(&capsule,/,
  'windows capsule must not use window-vibrancy Acrylic because it paints a rectangular grey host on Win11',
);

assertNotMatch(
  libRs,
  /DwmEnableBlurBehindWindow|DWMWA_SYSTEMBACKDROP_TYPE|SetWindowRgn/,
  'windows capsule must not use HWND-level DWM material or native regions; the DOM pill owns the visible shape',
);

assertMatch(
  libRs,
  /apply_acrylic\(&qa,\s*Some\(\(30,\s*32,\s*38,\s*140\)\)\)/,
  'windows QA window may keep Acrylic because its panel fills the native host',
);

assertMatch(
  capsuleLayoutTs,
  /return \{ width: 180, height: 44, textWidth: 88, boxSizing: 'border-box' \};[\s\S]*?const horizontalInset = 12;[\s\S]*?width: 220,[\s\S]*?height: translationActive \? 118 : 84,[\s\S]*?bottomInset: 12,/,
  'windows capsule should keep the original compact DOM pill inside a transparent native host',
);

assertMatch(
  capsuleTsx,
  /const useBackdrop = os !== 'win';[\s\S]*?background: os === 'win' \? 'rgba\(255, 255, 255, 0\.96\)' : 'rgba\(255, 255, 255, 0\.85\)'/,
  'windows capsule pill should use an opaque DOM surface instead of WebView2 backdrop-filter over a transparent host',
);

assertMatch(
  capsuleTsx,
  /return\s*\(\s*<div\s*style=\{\{[\s\S]*?width:\s*'100%',[\s\S]*?height:\s*'100%',[\s\S]*?paddingLeft:\s*hostMetrics\.horizontalInset,[\s\S]*?paddingRight:\s*hostMetrics\.horizontalInset,[\s\S]*?background:\s*'transparent'/,
  'capsule host should remain transparent outside the visible pill',
);
