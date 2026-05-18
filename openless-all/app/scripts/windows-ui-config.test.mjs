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
const tokensCss = await readFile(new URL('../src/styles/tokens.css', import.meta.url), 'utf-8');

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
assertEqual(mainWindow.decorations, true, 'shared main window config should keep native macOS traffic lights');
assertEqual(mainWindow.visible, false, 'windows main window should stay hidden until the intended first show point');

assertMatch(
  libRs,
  /#\[cfg\(target_os = "windows"\)\][\s\S]*?main\.set_decorations\(false\)/,
  'windows runtime should disable native chrome before the first show',
);

assertMatch(
  coordinatorRs,
  /#\[cfg\(target_os = "macos"\)\][\s\S]*?orderFrontRegardless/,
  'macOS capsule should show without taking the key window',
);

if (!/function WindowsResizeHandles\(\)/.test(windowChromeTsx)) {
  throw new Error('windows frameless shell should expose explicit resize handles');
}

assertMatch(
  windowChromeTsx,
  /const MAC_TITLEBAR_HEIGHT = 28;/,
  'macOS titlebar spacer should stay visually compact around the native traffic lights',
);
assertMatch(
  libRs,
  /show_main_window[\s\S]*?set_focus\(\)/,
  'macOS main window should rely on native traffic lights instead of manually moving standardWindowButton frames',
);
if (/standardWindowButton|setFrameOrigin: origin|tune_macos_main_window_controls/.test(libRs)) {
  throw new Error('macOS traffic lights should not be manually repositioned; keep native AppKit button frames visible');
}
if (!/action=\"close\"/.test(windowChromeTsx) || !/tone=\"danger\"/.test(windowChromeTsx)) {
  throw new Error('windows titlebar should keep the close button and danger hover treatment');
}
assertMatch(
  tokensCss,
  /--ol-motion-spring:[\s\S]*?--ol-motion-soft:[\s\S]*?--ol-motion-quick:/,
  'shared motion tokens should drive shell animations and transitions',
);

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

if (!/if \(os === 'win'\)\s*\{[\s\S]*?return \{ width: 180, height: 44, textWidth: 88, boxSizing: 'border-box' \};[\s\S]*?\}/.test(capsuleLayoutTs)) {
  throw new Error('windows capsule should keep the original compact visible pill metrics');
}

if (!/if \(os === 'win'\)\s*\{[\s\S]*?const horizontalInset = 12;[\s\S]*?width: 220,[\s\S]*?height: translationActive \? 118 : 84,[\s\S]*?horizontalInset,[\s\S]*?bottomInset: 12,[\s\S]*?badgeGap: 8,[\s\S]*?boxSizing: 'border-box',[\s\S]*?\}/.test(capsuleLayoutTs)) {
  throw new Error('windows capsule host metrics should leave room for shadow and badge geometry');
}

if (!/const hostMetrics = getCapsuleHostMetrics\(os,\s*translation\);/.test(capsuleTsx)) {
  throw new Error('capsule should derive host metrics from the shared layout contract');
}

if (!/return\s*\(\s*<div\s*style=\{\{[\s\S]*?width:\s*'100%',[\s\S]*?height:\s*'100%',[\s\S]*?position:\s*'relative',[\s\S]*?display:\s*'flex',[\s\S]*?alignItems:\s*'center',[\s\S]*?justifyContent:\s*'center',[\s\S]*?paddingLeft:\s*hostMetrics\.horizontalInset,[\s\S]*?paddingRight:\s*hostMetrics\.horizontalInset,[\s\S]*?\}\}/.test(capsuleTsx)) {
  throw new Error('capsule host should center the pill within the shared layout contract');
}

if (!/paddingLeft:\s*hostMetrics\.horizontalInset,/.test(capsuleTsx) || !/paddingRight:\s*hostMetrics\.horizontalInset,/.test(capsuleTsx)) {
  throw new Error('windows capsule host should reserve shared horizontal inset room for shadow geometry');
}

if (!/paddingBottom:\s*os === 'win' \? hostMetrics\.bottomInset : 0/.test(capsuleTsx)) {
  throw new Error('windows capsule host should respect the shared bottom inset');
}

if (!/hostMetrics\.bottomInset \+ metrics\.height \+ hostMetrics\.badgeGap/.test(capsuleTsx)) {
  throw new Error('windows translation badge should anchor from the shared host inset instead of a fixed center-based offset');
}

if (!/#\[cfg\(target_os = "windows"\)\]\s*\{[\s\S]*?const WINDOWS_CAPSULE_SIDE_INSET: f64 = 12\.0;[\s\S]*?width: 220\.0,[\s\S]*?height: if translation_active \{ 118\.0 \} else \{ 84\.0 \},[\s\S]*?bottom_inset: WINDOWS_CAPSULE_SIDE_INSET,/.test(libRs)) {
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
