import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import yaml from 'js-yaml';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8').replace(/\r/g, '');
const release = yaml.load(read('../../../.github/workflows/release-tauri.yml'));
const ci = yaml.load(read('../../../.github/workflows/ci.yml'));
const inputs = release.on.workflow_dispatch.inputs;
assert.equal(inputs.windows_backend_diagnostics.default, false);
assert.equal(inputs.windows_hang_diagnostics.default, false);
const gate = release.jobs.build.env.OPENLESS_BACKEND_DIAGNOSTICS;
for (const fragment of [
  'workflow_dispatch',
  'inputs.windows_backend_diagnostics',
  'windows-latest',
  'refs/heads/',
  "inputs.release_tag == ''",
]) {
  assert.ok(gate.includes(fragment));
}
const steps = release.jobs.build.steps;
const guard = steps.find((step) => step.name === 'Validate independent diagnostic sessions');
assert.ok(guard);
assert.ok(steps.indexOf(guard) < steps.findIndex((step) => step.name === 'Build (Windows)'));
assert.match(guard.run, /LEGACY_DIAGNOSTICS.*!= true/);
assert.match(guard.run, /-z "\$INPUT_RELEASE_TAG"/);
assert.match(guard.run, /refs\/heads\/\*/);
assert.match(guard.run, /INPUT_PLATFORM.*!= macos/);
assert.equal(
  steps.find((step) => step.uses === 'actions/checkout@v4').with.ref,
  '${{ inputs.release_tag || github.sha }}',
);
const build = steps.find((step) => step.name === 'Build (Windows)').run;
assert.match(build, /diagnostic_args=\(--features issue-1338-diagnostics\)/);
assert.match(build, /backend_observation=\$OPENLESS_BACKEND_DIAGNOSTICS/);
assert.equal((build.match(/build "\$\{diagnostic_args\[@\]\}" --bundles/g) ?? []).length, 4);
const provenance = steps.find((step) => step.name === 'Write Windows artifact provenance').run;
assert.match(provenance, /issue_1338_diagnostics = \(\$env:OPENLESS_BACKEND_DIAGNOSTICS -eq 'true'\)/);
assert.match(provenance, /issue-1338-20261106/);
const upload = steps.find((step) => step.name === 'Upload Windows artifacts');
assert.match(upload.with.name, /openless-windows-x64-issue-1338-diagnostics/);
assert.match(upload.with.path, /openless-provenance\.json/);
assert.match(upload.with.path, /openless-symbol-pair\.json/);
// Both observation modes remain branch-artifact paths, never publishing conditions.
assert.doesNotMatch(release.jobs.publish.if, /diagnostics/);
assert.match(release.jobs.publish.if, /refs\/tags\/v/);
assert.match(release.jobs.publish.if, /inputs.release_tag != ''/);
const ciSteps = Object.values(ci.jobs).flatMap((job) => job.steps ?? []);
assert.ok(
  ciSteps.some((step) => /cargo check.*--features issue-1338-diagnostics/.test(step.run ?? '')),
);
assert.ok(ciSteps.some((step) => /test-backend-hang-diagnostics\.py/.test(step.run ?? '')));
console.log('backend observation branch-build workflow contracts passed');
