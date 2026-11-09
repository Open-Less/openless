import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const workflows = {
  android: await readFile(
    new URL('../../../.github/workflows/android-apk.yml', import.meta.url),
    'utf8',
  ),
  ci: await readFile(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8'),
  tauriRelease: await readFile(
    new URL('../../../.github/workflows/release-tauri.yml', import.meta.url),
    'utf8',
  ),
};

const cancelSupersededTagPush = "cancel-in-progress: ${{ github.event_name == 'push' }}";

assert.ok(
  workflows.android.includes(
    "group: ${{ github.workflow }}-${{ github.ref }}-${{ github.event_name }}-${{ inputs.abis || 'aarch64' }}-${{ inputs.fast_profile || false }}-${{ inputs.signed_debug || false }}",
  ),
  'Android must group equivalent manual requests without mixing ABI, profile or signing modes',
);
assert.ok(
  workflows.android.includes(
    "cancel-in-progress: ${{ github.event_name == 'workflow_dispatch' && startsWith(github.ref, 'refs/heads/') }}",
  ),
  'Android may supersede manual branch builds, but never tag publishing (including manual tags)',
);
assert.ok(
  workflows.tauriRelease.includes(
    "group: ${{ github.workflow }}-${{ github.event_name == 'workflow_dispatch' && startsWith(github.ref, 'refs/heads/') && inputs.release_tag == '' && format('{0}-{1}-{2}', github.ref, inputs.platform, inputs.windows_hang_diagnostics) || format('{0}-{1}', github.ref, github.run_id) }}",
  ),
  'equivalent Tauri branch builds share a group; different platform/diagnostic requests are independent',
);
assert.ok(
  workflows.tauriRelease.includes(
    "cancel-in-progress: ${{ github.event_name == 'workflow_dispatch' && startsWith(github.ref, 'refs/heads/') && inputs.release_tag == '' }}",
  ),
  'only manual Tauri branch builds cancel; tag publishes and draft repairs must keep running',
);
assert.ok(
  !workflows.tauriRelease.includes(cancelSupersededTagPush),
  'Tauri tag publishes must not cancel an in-progress release',
);

assert.ok(
  workflows.ci.includes(
    "group: ${{ github.workflow }}-${{ github.ref }}-${{ github.event_name }}-${{ inputs.platform || 'all' }}-${{ inputs.macos_runner || 'macos-latest' }}",
  ),
  'CI must group equivalent requests without mixing event, platform or runner selections',
);
assert.ok(
  workflows.ci.includes(
    "cancel-in-progress: ${{ github.event_name == 'pull_request' || github.event_name == 'workflow_dispatch' }}",
  ),
  'CI must cancel superseded PR and manual checks without cancelling baseline branch pushes',
);

assert.ok(
  workflows.ci.includes('branches: [main, beta]'),
  'CI must run on default-branch pushes (baseline evidence and the only cache writer)',
);
assert.ok(
  !workflows.ci.includes("branches: ['**']"),
  'CI must not spend a second full matrix on feature-branch pushes that their pull request already covers',
);
// Release workflows are tag-only: a per-branch push rebuilt a 1-2 GB cache entry
// on every commit and evicted the beta scope, while `ci.yml` already builds the
// same trees for the quality gate. A weekly scheduled run keeps them warm.
for (const [name, source] of [
  ['Android release', workflows.android],
  ['Tauri release', workflows.tauriRelease],
]) {
  assert.doesNotMatch(
    source,
    /push:[\s\S]{0,200}branches:/,
    `${name} workflow must not rebuild on every branch push`,
  );
  assert.match(source, /schedule:/, `${name} workflow needs a warm-up schedule`);
  assert.match(source, /tags:[\s\S]{0,40}v\*-tauri/, `${name} workflow must still ship tags`);
}

console.log('workflow-concurrency-contract.test.mjs passed');
