import { readFile } from 'node:fs/promises';

function assertEqual(actual, expected, name) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

const raw = await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf-8');
const config = JSON.parse(raw);
const capsuleWindow = config.app.windows.find((window) => window.label === 'capsule');
const libRs = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8');
const coordinatorRs = await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8');

if (!capsuleWindow) {
  throw new Error('capsule window config missing');
}

assertEqual(capsuleWindow.width, 220, 'windows capsule config keeps translation-capable width baseline');
assertEqual(capsuleWindow.height, 110, 'windows capsule config keeps translation-capable height baseline');
assertEqual(capsuleWindow.transparent, true, 'capsule window should keep transparent visuals');
assertEqual(capsuleWindow.alwaysOnTop, true, 'capsule window should stay above the focused app while recording');
assertMatch(
  libRs,
  /#\[cfg\(target_os = "windows"\)\][\s\S]*?\(196\.0, height\)/,
  'windows runtime capsule width should collapse to the visible pill',
);
assertMatch(
  libRs,
  /let height = if translation_active \{ 110\.0 \} else \{ 52\.0 \};/,
  'windows runtime capsule height should shrink outside translation mode',
);
assertMatch(
  libRs,
  /window\.set_size\(LogicalSize::new\(cap_w, cap_h\)\)\?/,
  'capsule positioning should resync runtime size with the computed layout',
);
assertMatch(
  coordinatorRs,
  /let visible = matches!\(\s*state,\s*CapsuleState::Recording \| CapsuleState::Transcribing \| CapsuleState::Polishing\s*\);/m,
  'capsule should only stay visible during active recording or processing states',
);
assertMatch(
  coordinatorRs,
  /fn hide_capsule_window_if_present\(\)/,
  'windows capsule lifecycle should include an explicit native hide helper',
);
assertMatch(
  coordinatorRs,
  /ShowWindow\(hwnd, SW_HIDE\)/,
  'windows capsule hide helper should force the native window hidden',
);
assertMatch(
  coordinatorRs,
  /SetWindowPos\([\s\S]*?HWND_NOTOPMOST[\s\S]*?SWP_HIDEWINDOW/m,
  'windows capsule hide helper should drop topmost participation when inactive',
);
