import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import yaml from 'js-yaml';

const workflow = (name) =>
  yaml.load(readFileSync(new URL(`../../../.github/workflows/${name}`, import.meta.url), 'utf8'));
for (const name of ['release-tauri.yml', 'android-apk.yml']) {
  const steps = Object.values(workflow(name).jobs).flatMap((job) => job.steps ?? []);
  const publishers = steps.filter((step) => step.uses?.startsWith('softprops/action-gh-release@'));
  assert.equal(publishers.length, 1, `${name} must have one asset publisher`);
  assert.equal(
    publishers[0].with.draft,
    true,
    `${name}: no channel may publish a partially built release`,
  );
  assert.match(publishers[0].with.prerelease, /OPENLESS_RELEASE_CHANNEL == 'beta'/);
}
const sync = workflow('sync-homebrew-cask.yml');
assert.deepEqual(sync.on.release.types, ['published']);
assert.ok(sync.on.workflow_dispatch.inputs.tag.required);
const steps = sync.jobs['update-homebrew-cask'].steps;
const checkout = steps.find((step) => step.uses === 'actions/checkout@v4');
assert.match(checkout.with.token, /HOMEBREW_CASK_ADMIN_TOKEN/);
const commit = steps.find((step) => step.name === 'Commit & push cask bump');
assert.match(commit.env.CASK_ADMIN_TOKEN_AVAILABLE, /HOMEBREW_CASK_ADMIN_TOKEN/);
assert.ok(commit.run.indexOf('CASK_ADMIN_TOKEN_AVAILABLE') < commit.run.indexOf('git push'));
const gate = steps.find((step) => step.id === 'publication');
assert.ok(gate);
assert.match(gate.run, /--json isDraft,isPrerelease/);
assert.match(gate.run, /releases\/latest/);
assert.match(gate.run, /\[ "\$latest" != "\$TAG" \]/);
assert.match(gate.run, /\[ "\$private" != false \]/);
for (const name of [
  'Resolve version + DMG sha256 from the release',
  'Update Casks/openless.rb',
  'Commit & push cask bump',
]) {
  const step = steps.find((candidate) => candidate.name === name);
  assert.equal(
    step?.if,
    "steps.publication.outputs.ready == 'true'",
    `${name}: a draft must not change the public cask`,
  );
}
console.log('release-publication-contract.test.mjs passed');
