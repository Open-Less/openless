import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const all = fileURLToPath(new URL('../..', import.meta.url));
const scratch = mkdtempSync(join(tmpdir(), 'openless-harmony-audit-'));
try {
  for (const path of [
    'harmony/scripts/audit-harmony-ipc.mjs',
    'harmony/ipc-audit.json',
    'harmony/rust/src/lib.rs',
    'harmony/entry/src/main/ets/HarmonyApi.ets',
    'app/src/lib/ipc',
    'app/src/api/dictationReferences.ts',
    'app/src/api/in-app-dictation.ts',
    'app/src/runtime/files.ts',
    'app/src/runtime/platform.ts',
    'app/src/runtime/transport.ts',
  ]) {
    mkdirSync(dirname(join(scratch, path)), { recursive: true });
    cpSync(join(all, path), join(scratch, path), { recursive: true });
  }
  const report = join(scratch, 'harmony/ipc-audit.json');
  const check = () =>
    spawnSync(
      process.execPath,
      [join(scratch, 'harmony/scripts/audit-harmony-ipc.mjs'), '--check'],
      {
        cwd: join(scratch, 'app'),
        encoding: 'utf8',
        timeout: 5000,
      },
    );
  const original = readFileSync(report, 'utf8');
  assert.equal(check().status, 0);
  assert.equal(readFileSync(report, 'utf8'), original, 'check must never rewrite the valid report');
  writeFileSync(report, '{}\n');
  const stale = check();
  assert.ifError(stale.error);
  assert.notEqual(stale.status, 0, 'stale IPC reports must fail the gate');
  assert.match(stale.stderr, /Harmony IPC audit is stale/);
  assert.equal(readFileSync(report, 'utf8'), '{}\n', 'stale check must remain read-only');
  // A coordinated frontend removal must remove the retired permission command
  // from the generated audit, without pretending Harmony implements it.
  const referenceApi = join(scratch, 'app/src/api/dictationReferences.ts');
  writeFileSync(referenceApi, "invoke('get_dictation_references_status');\n");
  const refreshed = spawnSync(process.execPath, [join(scratch, 'harmony/scripts/audit-harmony-ipc.mjs')], { encoding: 'utf8' });
  assert.equal(refreshed.status, 0, refreshed.stderr);
  const commands = JSON.parse(readFileSync(report, 'utf8')).commands;
  assert.equal(commands.some(command => command.name === 'open_dictation_references_permission_settings'), false);
  assert.equal(commands.find(command => command.name === 'get_dictation_references_status').implementation, 'unimplemented');
  assert.equal(check().status, 0);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log('Harmony IPC audit --check rejects stale reports without writing (isolated fixture)');
