import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(
  new URL('../src-tauri/src/desktop_resources.rs', import.meta.url),
  'utf8',
);
const closeMain = source.match(/pub\(crate\) fn close_main[\s\S]*?\n}\n/)?.[0] ?? '';
assert.match(closeMain, /window\.hide\(\)/);
assert.match(closeMain, /window\.eval\("window\.stop\(\); location\.replace\('about:blank'\);"\)/);
assert.match(closeMain, /Duration::from_millis\(120\)/);
assert.match(closeMain, /window\.destroy\(\)/);
assert.match(closeMain, /location\.replace\('index\.html'\)/);
assert.match(closeMain, /reopen_pending/);
console.log('main WebView unload-before-destroy contract passed');
