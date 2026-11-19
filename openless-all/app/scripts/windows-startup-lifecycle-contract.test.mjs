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
const mainWindow = config.app.windows.find((window) => window.label === 'main');
const startup = await readFile(new URL('../src/app/startup.ts', import.meta.url), 'utf-8');

if (!mainWindow) {
  throw new Error('main window config missing');
}

assertEqual(
  mainWindow.visible,
  false,
  'main window should stay hidden until startup contract allows first show',
);

assertMatch(
  startup,
  /if \(os === 'win'\)[^]*?getHotkeyStatus\(\)[^]*?status\.state !== 'starting'\) return 'ready'/,
  'Windows startup should wait for hotkey status to leave the starting phase before entering ready',
);
