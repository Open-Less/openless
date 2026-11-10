import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import yaml from 'js-yaml';

const workflow = (name) =>
  yaml.load(readFileSync(new URL(`../../../.github/workflows/${name}`, import.meta.url), 'utf8'));
for (const name of ['release-tauri.yml', 'android-apk.yml']) {
  const steps = Object.values(workflow(name).jobs).flatMap((job) => job.steps ?? []);
  const publishers = steps.filter((step) => step.uses?.startsWith('softprops/action-gh-release@'));
  assert.equal(publishers.length, 1, `${name} must have one asset publisher`);
  assert.equal(publishers[0].with.draft, true, `${name}: never publish a partial release`);
  assert.match(publishers[0].with.prerelease, /OPENLESS_RELEASE_CHANNEL == 'beta'/);
  assert.doesNotMatch(publishers[0].with.files, /latest-[^\s]*\.json/);
  const manifestUploads = steps.filter(
    (step) =>
      step.uses?.startsWith('actions/upload-artifact@') &&
      /latest-[^\s]*\.json/.test(step.with?.path ?? ''),
  );
  assert.equal(manifestUploads.length, 1, `${name}: updater manifests belong to the CDN artifact`);
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
  assert.equal(
    steps.find((step) => step.name === name)?.if,
    "steps.publication.outputs.ready == 'true'",
    `${name}: drafts must not change the public cask`,
  );
}

const handoff = workflow('release-tauri.yml').jobs['dispatch-platform-releases'];
assert.equal(handoff.env.LINUX_ACCEPTED_RELEASE_TAG, undefined);
const dispatch = handoff.steps.find((step) =>
  step.run?.includes('gh workflow run release-harmony.yml'),
);
assert.ok(dispatch);
assert.doesNotMatch(JSON.stringify(handoff), /release-linux-egui|LINUX_ACCEPTED_RELEASE_TAG/);
const directory = mkdtempSync(join(tmpdir(), 'openless-release-handoff-'));
try {
  const calls = join(directory, 'calls');
  writeFileSync(join(directory, 'gh'), '#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALLS"\n', {
    mode: 0o755,
  });
  for (const [channel, expected] of [
    ['none', 0],
    ['release', 0],
    ['test', 0],
    ['invalid', 1],
  ]) {
    writeFileSync(calls, '');
    const result = spawnSync('bash', ['-c', dispatch.run], {
      env: {
        ...process.env,
        PATH: `${directory}:${process.env.PATH}`,
        CALLS: calls,
        RELEASE_TAG: 'v2.0.2-Beta.5-tauri',
        HARMONY_CHANNEL: channel,
      },
    });
    assert.equal(result.status, expected, channel);
    const logged = readFileSync(calls, 'utf8');
    if (expected === 0) {
      assert.match(
        logged,
        new RegExp(
          `release-harmony\\.yml.*--ref v2\\.0\\.2-Beta\\.5-tauri.*tag=v2\\.0\\.2-Beta\\.5-tauri.*channel=${channel}`,
        ),
      );
      assert.equal(logged.trim().split('\n').length, 1, 'dispatch only Harmony');
    } else {
      assert.equal(logged, '', 'bad channel must not dispatch');
    }
  }
} finally {
  rmSync(directory, { recursive: true, force: true });
}
console.log('release-publication-contract.test.mjs passed');
