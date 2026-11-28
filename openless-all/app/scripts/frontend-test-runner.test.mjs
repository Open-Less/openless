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
  'src/features/capsule/referenceBadgeView.test.ts',
  'src/features/settings/sections/dictationReferenceKey.test.ts',
  'src/features/settings/shell/navigation.test.ts',
  'src/lib/ipc/mock-data.test.ts',
  'scripts/dictation-references-contract.test.mjs',
]) {
  assert(discovered.includes(expected), `aggregate discovery omitted ${expected}`);
}
assert.equal(
  new Set(discovered).size,
  discovered.length,
  'aggregate discovery must not duplicate tests',
);

const invocations = [];
const statuses = [0, 7, 3, 0];
const exitCode = runTestFiles(
  ['src/lib/passes.test.ts', 'scripts/fails.test.mjs', 'scripts/also-fails.test.mjs', 'scripts/still-runs.test.mjs'],
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

assert.equal(exitCode, 7, 'the aggregate runner must preserve the first failing child status');
assert.equal(invocations.length, 4, 'the aggregate runner must execute every test despite failures');
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

for (const failure of [{ error: { code: 'ETIMEDOUT' } }, { error: new Error('spawn failed') }, { status: null }]) {
  let calls = 0;
  const result = runTestFiles(['scripts/fails.test.mjs', 'scripts/still-runs.test.mjs'], {
    appRoot: '/app', log: () => {},
    spawn: () => (++calls === 1 ? failure : { status: 0 }),
  });
  assert.equal(result, failure.error?.code === 'ETIMEDOUT' ? 124 : 1);
  assert.equal(calls, 2, 'startup failures and timeouts must not hide later tests');
}

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
