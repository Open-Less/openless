import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const workflows = {
  android: await readFile(new URL('../../../.github/workflows/android-apk.yml', import.meta.url), 'utf8'),
  ci: await readFile(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8'),
  tauriRelease: await readFile(
    new URL('../../../.github/workflows/release-tauri.yml', import.meta.url),
    'utf8',
  ),
};

const tagGroup =
  "group: ${{ github.workflow }}-${{ github.event_name == 'workflow_dispatch' && github.run_id || github.ref }}";
const cancelSupersededTagPush = "cancel-in-progress: ${{ github.event_name == 'push' }}";

for (const [name, source] of [
  ['Android release', workflows.android],
  ['Tauri release', workflows.tauriRelease],
]) {
  assert.ok(
    source.includes(tagGroup),
    `${name} workflow must isolate manual runs while grouping repeated pushes of the same tag`,
  );
  assert.ok(
    source.includes(cancelSupersededTagPush),
    `${name} workflow must cancel an older run when the same tag is pushed again`,
  );
}

assert.ok(
  workflows.ci.includes('group: ${{ github.workflow }}-${{ github.ref }}'),
  'PR CI must group runs by workflow and ref',
);
assert.ok(
  workflows.ci.includes(
    "cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
  ),
  'PR CI must cancel superseded pull-request runs without cancelling branch pushes',
);

console.log('workflow-concurrency-contract.test.mjs passed');
