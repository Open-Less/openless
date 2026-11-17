import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import process from 'node:process';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';

import { discoverTestFiles, runTestFiles } from './frontend-test-runner.mjs';

const discovered = discoverTestFiles();
for (const expected of [
  'scripts/check-android-updater-pubkey.mjs',
  'scripts/macos-capsule-spaces-contract.test.mjs',
  'scripts/macos-audio-usage-description-contract.test.mjs',
  'scripts/repository-owner-contract.test.mjs',
  'scripts/windows-ui-config.test.mjs',
  'src/lib/hotkeySideModifiers.test.ts',
  'src/lib/windowHotkeyFallback.test.ts',
]) {
  assert(discovered.includes(expected), `aggregate discovery omitted ${expected}`);
}
assert.equal(
  new Set(discovered).size,
  discovered.length,
  'aggregate discovery must not duplicate tests',
);

const invocations = [];
const statuses = [0, 7, 0];
const exitCode = runTestFiles(
  ['src/lib/passes.test.ts', 'scripts/fails.test.mjs', 'scripts/must-not-run.test.mjs'],
  {
    appRoot: '/app',
    log: () => {},
    spawn(command, args, options) {
      invocations.push({ command, args, options });
      return { status: statuses[invocations.length - 1] };
    },
    tsxImport: 'file:///tools/tsx-loader.mjs',
  },
);

assert.equal(exitCode, 7, 'the aggregate runner must propagate a failing child status');
assert.equal(invocations.length, 2, 'the aggregate runner must stop after the first failure');
assert.deepEqual(invocations[0].args, [
  '--import',
  'file:///tools/tsx-loader.mjs',
  resolve('/app', 'src/lib/passes.test.ts'),
]);
assert.deepEqual(invocations[1].args, [resolve('/app', 'scripts/fails.test.mjs')]);
assert.equal(invocations[0].command, process.execPath);
assert.equal(invocations[1].command, process.execPath);
assert.equal(invocations[0].options.stdio, 'inherit');
assert.equal(invocations[0].options.timeout, 120_000);

// Exercise a real child that never exits, including one that ignores SIGTERM.
const scratch = mkdtempSync(resolve(tmpdir(), 'openless-test-timeout-'));
try {
  writeFileSync(
    resolve(scratch, 'hang.mjs'),
    "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000);",
  );
  const messages = [];
  const started = performance.now();
  assert.equal(
    runTestFiles(['hang.mjs'], {
      appRoot: scratch,
      timeout: 250,
      log: (message) => messages.push(message),
    }),
    124,
  );
  assert.ok(performance.now() - started < 5000, 'a hung test must not hold the suite');
  assert.ok(messages.some((message) => /timed out after 250ms: hang.mjs/.test(message)));

  // A TypeScript child must also be killed, rather than orphaned behind the
  // tsx CLI. A delayed write would expose a still-running grandchild.
  const marker = resolve(scratch, 'orphan.txt');
  writeFileSync(
    resolve(scratch, 'hang.test.ts'),
    `import { writeFileSync } from 'node:fs';
const marker: string = ${JSON.stringify(marker)};
process.on('SIGTERM', () => {});
setTimeout(() => writeFileSync(marker, 'orphan'), 2500);
setInterval(() => {}, 1000);`,
  );
  assert.equal(
    runTestFiles(['hang.test.ts'], { appRoot: scratch, timeout: 1500, log: () => {} }),
    124,
  );
  await new Promise((done) => setTimeout(done, 3000));
  assert.equal(existsSync(marker), false, 'timing out must terminate the TypeScript test');
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

console.log('frontend-test-runner.test.mjs passed');
