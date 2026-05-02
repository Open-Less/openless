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
const mainWindow = config.app.windows.find((window) => window.label === 'main');
const libRs = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8');
const coordinatorRs = await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8');
const capsuleTsx = await readFile(new URL('../src/components/Capsule.tsx', import.meta.url), 'utf-8');
const capsuleLayoutTs = await readFile(new URL('../src/lib/capsuleLayout.ts', import.meta.url), 'utf-8');
const windowChromeTsx = await readFile(new URL('../src/components/WindowChrome.tsx', import.meta.url), 'utf-8');
const floatingShellTsx = await readFile(new URL('../src/components/FloatingShell.tsx', import.meta.url), 'utf-8');

if (!capsuleWindow) {
  throw new Error('capsule window config missing');
}
if (!mainWindow) {
  throw new Error('main window config missing');
}
assertEqual(capsuleWindow.width, 220, 'windows capsule config keeps translation-capable width baseline');
assertEqual(capsuleWindow.height, 110, 'windows capsule config keeps translation-capable height baseline');
assertEqual(capsuleWindow.transparent, true, 'capsule window should keep transparent visuals');
assertEqual(capsuleWindow.alwaysOnTop, true, 'capsule window should stay above the focused app while recording');
assertEqual(mainWindow.decorations, true, 'shared main window config should preserve platform-native titlebars by default');
assertEqual(mainWindow.visible, false, 'windows main window should stay hidden until the intended first show point');

if (!/fn configure_main_window_for_platform<R: Runtime>\(window: &tauri::WebviewWindow<R>\)[\s\S]*?#\[cfg\(target_os = "windows"\)\][\s\S]*?window\.set_decorations\(false\)/.test(libRs)) {
  throw new Error('windows runtime should own the frameless shell switch instead of the shared config layer');
}

if (!/function WindowsResizeHandles\(\)/.test(windowChromeTsx)) {
  throw new Error('windows frameless shell should expose explicit resize handles');
}

if (!/startResizeDragging\(direction\)/.test(windowChromeTsx)) {
  throw new Error('windows resize handles should delegate edge dragging to Tauri');
}

if (!/borderRadius:\s*'var\(--ol-window-console-radius\)'/.test(floatingShellTsx)) {
  throw new Error('floating shell should consume the shared window-console radius');
}

assertMatch(
  coordinatorRs,
  /let visible = !matches!\(state,\s*CapsuleState::Idle\);/,
  'capsule should stay visible until the unified idle hide path runs',
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

if (!/export function getCapsuleHostMetrics\(\s*os: OS,\s*translationActive: boolean,\s*\): CapsuleHostMetrics/.test(capsuleLayoutTs)) {
  throw new Error('capsule layout should define explicit host metrics separate from the visible pill metrics');
}

if (!/if \(os === 'win'\)\s*\{[\s\S]*?width: 220,[\s\S]*?height: translationActive \? 118 : 84,[\s\S]*?bottomInset: 12,[\s\S]*?badgeGap: 8[\s\S]*?\}/.test(capsuleLayoutTs)) {
  throw new Error('windows capsule host metrics should leave room for shadow and badge geometry');
}

if (!/const hostMetrics = getCapsuleHostMetrics\(os,\s*translation\);/.test(capsuleTsx)) {
  throw new Error('capsule should derive host metrics from the shared layout contract');
}

if (!/justifyContent:\s*os === 'win' \? 'flex-end' : 'center'/.test(capsuleTsx)) {
  throw new Error('windows capsule host should anchor the pill to the bottom instead of centering it inside the larger native host window');
}

if (!/paddingBottom:\s*os === 'win' \? hostMetrics\.bottomInset : 0/.test(capsuleTsx)) {
  throw new Error('windows capsule host should respect the shared bottom inset');
}

if (!/bottom:\s*`\$\{hostMetrics\.bottomInset \+ metrics\.height \+ hostMetrics\.badgeGap\}px`/.test(capsuleTsx)) {
  throw new Error('windows translation badge should anchor from the shared host inset instead of a fixed center-based offset');
}

if (!/#\[cfg\(target_os = "windows"\)\][\s\S]*?width: 220\.0[\s\S]*?height: if translation_active \{ 118\.0 \} else \{ 84\.0 \}[\s\S]*?bottom_inset: 12\.0,/.test(libRs)) {
  throw new Error('windows runtime capsule bounds should leave room for the native shadow while keeping a fixed visual pill');
}

if (!/#\[cfg\(target_os = "windows"\)\]\s*\{\s*52\.0\s*\}/.test(libRs)) {
  throw new Error('windows capsule visual pill height should stay at 52px');
}

if (!/window\.set_size\(LogicalSize::new\(bounds\.width, bounds\.height\)\)\?/.test(libRs)) {
  throw new Error('capsule positioning should resync runtime size with the computed layout');
}

if (!/let _ = window\.hide\(\);/.test(coordinatorRs)) {
  throw new Error('capsule should be hidden once it leaves active states');
}
