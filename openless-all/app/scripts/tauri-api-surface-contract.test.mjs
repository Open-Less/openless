import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const config = JSON.parse(
  await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
);
const lifecycleE2e = await readFile(
  new URL('./windows-openless-lifecycle-e2e.py', import.meta.url),
  'utf8',
);
const miscCommands = await readFile(
  new URL('../src-tauri/src/commands/misc.rs', import.meta.url),
  'utf8',
);

assert.equal(
  config.app.withGlobalTauri,
  false,
  'the application must not expose the global Tauri API bundle',
);
assert.match(
  lifecycleE2e,
  /window\.__TAURI_INTERNALS__\.invoke\(/,
  'the Windows lifecycle E2E must invoke through the Tauri IPC bridge',
);
assert.doesNotMatch(
  lifecycleE2e,
  /window\.__TAURI__\./,
  'the Windows lifecycle E2E must not depend on the disabled global Tauri API',
);
assert.match(
  miscCommands,
  /window\.__TAURI_INTERNALS__\.invoke\(/,
  'the cursor-context debug example must use the available Tauri IPC bridge',
);
assert.doesNotMatch(
  miscCommands,
  /\b__TAURI__\./,
  'debug documentation must not recommend the disabled global Tauri API',
);

console.log('tauri-api-surface-contract.test.mjs passed');
