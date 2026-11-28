import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { withCargoFixtureCleanup } from './ci-cargo-fixture-cleanup.mjs';

// This regression never starts Cargo, a compiler, or any other child process.
const root = mkdtempSync(join(tmpdir(), 'openless-cargo-cleanup-contract-'));
const fixture = (name) => {
  const directory = join(root, name);
  mkdirSync(directory);
  writeFileSync(join(directory, 'fixture.txt'), 'fixture');
  return directory;
};
try {
  const success = fixture('success');
  assert.equal(withCargoFixtureCleanup(success, () => 42), 42);
  assert.equal(existsSync(success), false);

  const primary = new Error('fixture compilation or assertion failed');
  const failed = fixture('primary-failure');
  assert.throws(
    () =>
      withCargoFixtureCleanup(failed, () => {
        throw primary;
      }),
    (error) => error === primary,
  );
  assert.equal(existsSync(failed), false, 'the original failure must still clean its fixture');

  const cleanupError = Object.assign(new Error('fixture removal busy'), { code: 'EBUSY' });
  const cannotRemove = (directory, options) => {
    assert.equal(directory.startsWith(root), true);
    assert.deepEqual(options, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    // Simulate rmSync exhausting its own retry budget; do not emulate Windows locks.
    throw cleanupError;
  };
  const cleanupOnly = fixture('cleanup-failure');
  assert.throws(
    () => withCargoFixtureCleanup(cleanupOnly, () => 42, cannotRemove),
    (error) => error === cleanupError,
  );
  assert.equal(existsSync(cleanupOnly), true);

  const both = fixture('both-failures');
  assert.throws(
    () =>
      withCargoFixtureCleanup(
        both,
        () => {
          throw primary;
        },
        cannotRemove,
      ),
    (error) => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.cause, primary);
      assert.deepEqual(error.errors, [primary, cleanupError]);
      return true;
    },
  );
  assert.equal(existsSync(both), true);
} finally {
  rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
console.log('Windows Cargo fixture cleanup contract passed (no compilation)');
