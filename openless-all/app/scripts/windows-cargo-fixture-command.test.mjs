import assert from 'node:assert/strict';
import { createCargoFixtureRunner } from './ci-cargo-fixture-command.mjs';
import { withCargoFixtureCleanup } from './ci-cargo-fixture-cleanup.mjs';

// Inject spawn results: never start Cargo or any child process in this regression.
for (const platform of ['win32', 'darwin', 'linux']) {
  const calls = [];
  const messages = [];
  const timeoutError = Object.assign(new Error('spawnSync cargo ETIMEDOUT'), { code: 'ETIMEDOUT' });
  const results = [
    { status: 0, stdout: 'compiled', stderr: '' },
    {
      error: timeoutError,
      status: null,
      signal: 'SIGTERM',
      stdout: 'partial compiler output',
      stderr: 'fixture diagnostics',
    },
    { status: 101, stdout: '', stderr: 'production diagnostics checked' },
  ];
  const directory = 'isolated-fixture';
  const env = { CARGO_TARGET_DIR: 'isolated-fixture/host/target' };
  const cargo = createCargoFixtureRunner(directory, env, {
    platform,
    spawn: (command, args, options) => {
      calls.push({ command, args, options });
      return results[calls.length - 1];
    },
    report: (message) => messages.push(message),
  });
  const args = ['test', '--manifest-path', 'host/Cargo.toml', '--lib', '--bins', '--no-run'];
  assert.equal(cargo(args), results[0]);
  assert.equal(cargo(['build']), results[1], 'a timeout must not be retried or replaced');
  assert.equal(cargo(['test']), results[2], 'a compiler failure must not turn into success');
  assert.equal(results[1].error, timeoutError);
  assert.equal(calls.length, 3);
  assert.deepEqual(
    calls.map((call) => call.options.timeout),
    platform === 'win32' ? [45_000, 30_000, 30_000] : [30_000, 30_000, 30_000],
  );
  assert.deepEqual(calls[0].args, [...args, '--offline']);
  assert.deepEqual(args, ['test', '--manifest-path', 'host/Cargo.toml', '--lib', '--bins', '--no-run']);
  for (const call of calls) {
    assert.equal(call.command, 'cargo');
    assert.equal(call.options.cwd, directory);
    assert.equal(call.options.env, env);
    assert.equal(call.options.encoding, 'utf8');
    assert.equal(call.args.at(-1), '--offline');
    assert.equal(call.options.shell, undefined, 'keep direct child-process ownership');
  }
  assert.equal(messages.length, 1);
  assert.deepEqual(JSON.parse(messages[0].slice('[cargo-fixture] '.length)), {
    args: ['build', '--offline'],
    timeout: 30_000,
    code: 'ETIMEDOUT',
    status: null,
    signal: 'SIGTERM',
    stdout: 'partial compiler output',
    stderr: 'fixture diagnostics',
  });
}

const firstTimeout = Object.assign(new Error('spawnSync cargo ETIMEDOUT'), { code: 'ETIMEDOUT' });
const cleanupBusy = Object.assign(new Error('fixture still busy'), { code: 'EBUSY' });
const diagnostics = [];
const coldCargo = createCargoFixtureRunner('isolated-fixture', {}, {
  platform: 'win32',
  spawn: () => ({ error: firstTimeout, status: null, stdout: '', stderr: 'cold link output' }),
  report: (message) => diagnostics.push(message),
});
assert.throws(
  () =>
    withCargoFixtureCleanup(
      'isolated-fixture',
      () => assert.ifError(coldCargo(['test']).error),
      () => {
        throw cleanupBusy;
      },
    ),
  (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors[0].actual, firstTimeout);
    assert.equal(error.cause, error.errors[0]);
    assert.equal(error.errors[1], cleanupBusy);
    return true;
  },
);
assert.equal(JSON.parse(diagnostics[0].slice('[cargo-fixture] '.length)).timeout, 45_000);
console.log('Windows Cargo fixture command contract passed (mock spawn only)');
