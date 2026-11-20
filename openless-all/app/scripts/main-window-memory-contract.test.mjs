import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const closePath = source.match(/fn hide_main_window[\s\S]*?\n}\n/)?.[0] ?? '';
const lifecycle = await readFile(
  new URL('../src-tauri/src/desktop_resources.rs', import.meta.url),
  'utf8',
);
assert.match(
  closePath,
  /#\[cfg\(target_os = "macos"\)\]\s*desktop_resources::hide_main_with_grace\(app\)/,
);
assert.match(closePath, /desktop_resources::close_main\(app\)/);
assert.match(lifecycle, /sleep\(MAIN_CLOSE_GRACE\)[\s\S]*?pending == token[\s\S]*?close_main/);
assert.match(lifecycle, /window\.eval\("window\.stop\(\); location\.replace\('about:blank'\);"\)/);
assert.match(lifecycle, /window\.destroy\(\)/);
console.log('macOS reuses quick reopens and releases the WebView when the grace expires');
