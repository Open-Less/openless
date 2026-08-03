import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import process from 'node:process';

import { discoverTestFiles, runTestFiles } from './frontend-test-runner.mjs';

const discovered = discoverTestFiles();
for (const expected of [
  'scripts/check-android-updater-pubkey.mjs',
  'scripts/check-hotkey-injection.mjs',
  'scripts/less-computer-opencode-contract.test.mjs',
  'scripts/macos-capsule-spaces-contract.test.mjs',
  'scripts/macos-speech-usage-description-contract.test.mjs',
  'scripts/repository-owner-contract.test.mjs',
  'scripts/windows-ui-config.test.mjs',
  'src/lib/hotkeyRecorder.test.ts',
  'src/lib/windowHotkeyFallback.test.ts',
]) {
  assert(discovered.includes(expected), `aggregate discovery omitted ${expected}`);
}
assert.equal(new Set(discovered).size, discovered.length, 'aggregate discovery must not duplicate tests');

const invocations = [];
const statuses = [0, 7, 0];
const exitCode = runTestFiles(
  [
    'src/lib/passes.test.ts',
    'scripts/fails.test.mjs',
    'scripts/must-not-run.test.mjs',
  ],
  {
    appRoot: '/app',
    log: () => {},
    spawn(command, args, options) {
      invocations.push({ command, args, options });
      return { status: statuses[invocations.length - 1] };
    },
    tsxCli: '/tools/tsx-cli.mjs',
  },
);

assert.equal(exitCode, 7, 'the aggregate runner must propagate a failing child status');
assert.equal(invocations.length, 2, 'the aggregate runner must stop after the first failure');
assert.deepEqual(invocations[0].args, [
  '/tools/tsx-cli.mjs',
  resolve('/app', 'src/lib/passes.test.ts'),
]);
assert.deepEqual(invocations[1].args, [resolve('/app', 'scripts/fails.test.mjs')]);
assert.equal(invocations[0].command, process.execPath);
assert.equal(invocations[1].command, process.execPath);
assert.equal(invocations[0].options.stdio, 'inherit');

console.log('frontend-test-runner.test.mjs passed');
