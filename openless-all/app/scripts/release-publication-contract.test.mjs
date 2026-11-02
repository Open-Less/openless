import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import yaml from 'js-yaml';
import vm from 'node:vm';

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
  // Updater manifests belong to the CDN, not to the release assets: current clients read
  // `/updates/{stable,beta}/` (see src-tauri/src/update_source.rs), and cloning them into the
  // release produced fourteen near-identical files per tag.
  assert.doesNotMatch(
    publishers[0].with.files,
    /latest-[^\s]*\.json/,
    `${name}: updater manifests must not be attached to the release`,
  );
  const manifestUploads = steps.filter(
    (step) =>
      step.uses?.startsWith('actions/upload-artifact@') &&
      /latest-[^\s]*\.json/.test(step.with?.path ?? ''),
  );
  assert.equal(
    manifestUploads.length,
    1,
    `${name}: exactly one artifact must carry the updater manifests for the CDN sync`,
  );
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

const linux = workflow('check-linux-egui.yml');
const publish = linux.jobs['publish-linux-egui'];
assert.deepEqual(publish.needs, ['test-linux-egui', 'build-linux-egui']);
assert.equal(linux.jobs['test-linux-egui'].needs, undefined, 'testing must remain parallel');
assert.equal(linux.jobs['build-linux-egui'].needs, undefined, 'packaging must remain parallel');
assert.equal(publish.permissions, undefined, 'the reusable job must inherit its caller permission');
assert.equal(publish.env.GH_REPO, '${{ github.repository }}');
assert.equal(publish.env.GH_TOKEN, '${{ github.token }}');
assert.equal(publish.env.RELEASE_TAG, '${{ inputs.release_tag }}');
assert.doesNotMatch(publish.if, /always\(|failure\(|cancelled\(/);
for (const [results, inputs, expected] of [
  [['success', 'success'], { scope: 'full', publish: true, release_tag: 'v2.0.2-tauri' }, true],
  [['failure', 'success'], { scope: 'full', publish: true, release_tag: 'v2.0.2-tauri' }, false],
  [['success', 'failure'], { scope: 'full', publish: true, release_tag: 'v2.0.2-tauri' }, false],
  [['cancelled', 'success'], { scope: 'full', publish: true, release_tag: 'v2.0.2-tauri' }, false],
  [['success', 'skipped'], { scope: 'full', publish: true, release_tag: 'v2.0.2-tauri' }, false],
  [['success', 'success'], { scope: 'none', publish: true, release_tag: 'v2.0.2-tauri' }, false],
  [['success', 'success'], { scope: 'full', publish: false, release_tag: 'v2.0.2-tauri' }, false],
  [['success', 'success'], { scope: 'full', publish: true, release_tag: '' }, false],
]) {
  const enabled =
    results.every((result) => result === 'success') &&
    Boolean(vm.runInNewContext(publish.if, { inputs }));
  assert.equal(enabled, expected, JSON.stringify({ results, inputs }));
}
const downloads = publish.steps.filter((step) => step.uses === 'actions/download-artifact@v8');
assert.deepEqual(
  downloads.map((step) => step.with.name),
  ['openless-linux-egui-x86_64', 'openless-linux-egui-direct-download'],
);
const uploads = linux.jobs['build-linux-egui'].steps.filter((step) =>
  step.uses?.startsWith('actions/upload-artifact@'),
);
for (const step of downloads) {
  assert.ok(uploads.some((upload) => upload.with.name === step.with.name));
  assert.equal(step.with['run-id'], undefined, 'download only this run');
  assert.equal(step.with['github-token'], undefined, 'do not enable cross-run downloads');
}
assert.equal(
  publish.steps.some((step) => step.uses?.startsWith('actions/checkout@')),
  false,
);
for (const [name, job] of Object.entries(linux.jobs)) {
  if (name !== 'publish-linux-egui')
    assert.ok((job.steps ?? []).every((step) => !step.run?.includes('gh release upload')));
}
const attach = publish.steps.find((step) => step.run?.includes('gh release upload'));
assert.match(attach.run, /seq 1 40/);
assert.match(attach.run, /sleep 15/);
assert.match(attach.run, /--clobber/);
console.log('release-publication-contract.test.mjs passed');
