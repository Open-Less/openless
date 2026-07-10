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
const mainWindow = config.app.windows.find(window => window.label === 'main');
const appTsx = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf-8');

if (!mainWindow) {
  throw new Error('main window config missing');
}

assertEqual(mainWindow.visible, false, 'main window should stay hidden until startup contract allows first show');

// Windows 走 while 循环轮询 hotkey 状态，等到 state !== 'starting' 再 setGate('ready')。
// 该路径在 if (os === 'win') 分支内，使用内联循环而非独立函数。
assertMatch(
  appTsx,
  /if \(os === 'win'\)/,
  'windows startup gate should branch on os === win',
);
assertMatch(
  appTsx,
  /status\.state !== 'starting'[\s\S]*?setGate\('ready'\)/m,
  'windows startup should wait for hotkey status to leave the starting phase before entering ready',
);
