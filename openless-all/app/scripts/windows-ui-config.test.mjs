import { readFile } from 'node:fs/promises';

function assertEqual(actual, expected, name) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

const raw = await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf-8');
const config = JSON.parse(raw);
const mainWindow = config.app.windows.find((window) => window.label === 'main');
const windowChromeTsx = await readFile(new URL('../src/components/WindowChrome.tsx', import.meta.url), 'utf-8');
const floatingShellTsx = await readFile(new URL('../src/components/FloatingShell.tsx', import.meta.url), 'utf-8');

if (!mainWindow) {
  throw new Error('main window config missing');
}

assertEqual(mainWindow.decorations, false, 'windows main window should use only custom titlebar');
assertEqual(mainWindow.visible, false, 'windows main window should stay hidden until the intended first show point');

if (!/function WindowsResizeHandles\(\)/.test(windowChromeTsx)) {
  throw new Error('windows frameless shell should expose explicit resize handles');
}

if (!/startResizeDragging\(direction\)/.test(windowChromeTsx)) {
  throw new Error('windows resize handles should delegate edge dragging to Tauri');
}

if (!/borderRadius:\s*'var\(--ol-window-console-radius\)'/.test(floatingShellTsx)) {
  throw new Error('floating shell should consume the shared window-console radius');
}
