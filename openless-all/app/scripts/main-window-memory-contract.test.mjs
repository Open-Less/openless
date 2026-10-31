import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const closePath = source.match(/fn hide_main_window[\s\S]*?\n}\n/)?.[0] ?? '';
assert.match(closePath, /desktop_resources::close_main\(app\)/);
assert.doesNotMatch(closePath, /hide_main_with_grace/);
assert.match(
  closePath,
  /WebKit can release its content process and memory[\s\S]*?ensure_main/,
  'closing the main surface must explain and preserve the recreate path',
);
console.log('main window closes by destroying its WebView for memory release');
