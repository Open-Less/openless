import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, mkdirSync, readdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import yaml from 'js-yaml';
import { prepareRelease } from './prepare-release.mjs';

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
const desktop = workflow('release-tauri.yml');
assert.doesNotMatch(
  JSON.stringify(desktop),
  /stage-release-download-aliases|openless-download-alias/,
);
const staging = mkdtempSync(join(tmpdir(), 'openless-release-assets-'));
try {
  const source = join(staging, 'openless-release-artifacts', 'desktop');
  mkdirSync(source, { recursive: true });
  const originals = [
    'OpenLess_2.0.2-Beta.7_x64-setup.exe',
    'OpenLess_2.0.2-Beta.7_x64-setup.exe.sig',
    'OpenLess_2.0.2-Beta.7_aarch64.dmg',
    'OpenLess_aarch64.app.tar.gz',
    'OpenLess_aarch64.app.tar.gz.sig',
  ];
  // Old artifacts can survive a partial rerun: do not publish their aliases again.
  const excluded = [
    'OpenLess_windows_x86_64-setup.exe',
    'OpenLess_windows_x86_64-setup.exe.sig',
    'OpenLess_macos_aarch64.dmg',
    'latest-darwin-aarch64.json',
    'openless.exe',
    'debug.pdb',
  ];
  for (const name of [...originals, ...excluded]) writeFileSync(join(source, name), 'fixture');
  const stage = Object.values(desktop.jobs)
    .flatMap((job) => job.steps ?? [])
    .find((step) => step.name === 'Stage release files');
  assert.ok(stage);
  const result = spawnSync('bash', ['-c', stage.run], {
    env: { ...process.env, RUNNER_TEMP: staging, GITHUB_WORKSPACE: staging },
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(readdirSync(join(staging, 'release-files')).sort(), originals.sort());
} finally {
  rmSync(staging, { recursive: true, force: true });
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

const unified = workflow('release-tauri.yml');
assert.deepEqual(unified.on.push.tags, ['v*-tauri']);
assert.ok(unified.jobs['prepare-release']);
assert.match(unified.jobs['prepare-release'].if, /github.event_name == 'push'/);
assert.equal(unified.jobs['prepare-release'].permissions.contents, 'write');
assert.ok(
  unified.jobs['prepare-release'].steps.some((step) =>
    step.run?.includes('scripts/prepare-release.mjs'),
  ),
);
for (const name of ['build', 'android', 'harmony']) {
  assert.equal(
    unified.jobs[name].needs,
    'prepare-release',
    `${name} starts after the same short preflight`,
  );
}
assert.match(
  unified.jobs.build.if,
  /result == 'skipped'/,
  'manual/scheduled desktop builds still work',
);
for (const [name, file] of [
  ['android', 'android-apk.yml'],
  ['harmony', 'release-harmony.yml'],
]) {
  const job = unified.jobs[name];
  assert.equal(job.uses, `./.github/workflows/${file}`);
  assert.equal(job.secrets, 'inherit');
  assert.match(job.if, /github.event_name == 'push'/);
  assert.match(job.if, /refs\/tags\/v/);
  assert.equal(job.permissions.contents, 'write');
  assert.ok(workflow(file).on.workflow_call);
}
assert.equal(unified.jobs['dispatch-platform-releases'], undefined);
const desktopDownloads = unified.jobs.publish.steps.filter((step) =>
  step.uses?.startsWith('actions/download-artifact@'),
);
assert.deepEqual(desktopDownloads.map((step) => step.with.pattern).sort(), [
  'openless-macos-*',
  'openless-windows-*',
]);
const channelValidation = workflow('release-harmony.yml').jobs['build-harmony-hap'].steps.find(
  (step) => step.name === 'Validate release channel',
);
assert.ok(channelValidation);
assert.doesNotMatch(JSON.stringify(unified), /release-linux-egui|LINUX_ACCEPTED_RELEASE_TAG/);
const tag = 'v2.0.2-Beta.7-tauri';
const repository = 'Open-Less/openless';
const apiCalls = [];
const missing = (args) => {
  apiCalls.push(args);
  if (args[0] === 'api') throw Object.assign(new Error('not found'), { stderr: 'HTTP 404' });
  return '';
};
assert.equal(prepareRelease(tag, repository, missing), 'created');
assert.equal(apiCalls.length, 2);
for (const flag of ['--verify-tag', '--draft', '--prerelease'])
  assert.ok(apiCalls[1].includes(flag));
const stableCalls = [];
prepareRelease('v2.0.2-tauri', repository, (args) => {
  stableCalls.push(args);
  if (args[0] === 'api') throw Object.assign(new Error('not found'), { stderr: 'HTTP 404' });
  return '';
});
assert.ok(!stableCalls[1].includes('--prerelease'));
let reads = 0;
const existing = { tag_name: tag, draft: true, prerelease: true };
assert.equal(
  prepareRelease(tag, repository, () => {
    reads++;
    return JSON.stringify(existing);
  }),
  'existing draft',
);
assert.equal(reads, 1, 'retry reads the draft without changing its body or assets');
assert.throws(
  () => prepareRelease(tag, repository, () => JSON.stringify({ ...existing, draft: false })),
  /already published/,
);
assert.throws(
  () =>
    prepareRelease(tag, repository, () =>
      JSON.stringify({ ...existing, tag_name: 'v2.0.1-tauri' }),
    ),
  /tag mismatch/,
);
assert.throws(
  () => prepareRelease(tag, repository, () => JSON.stringify({ ...existing, prerelease: false })),
  /channel mismatch/,
);
let unavailableCalls = 0;
assert.throws(
  () =>
    prepareRelease(tag, repository, () => {
      unavailableCalls++;
      throw Object.assign(new Error('API unavailable'), { stderr: 'HTTP 503' });
    }),
  /API unavailable/,
);
assert.equal(unavailableCalls, 1, 'API errors cannot trigger release creation');
assert.throws(
  () => prepareRelease('main', repository, () => assert.fail('invalid tags cannot call GitHub')),
  /immutable/,
);
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
    const result = spawnSync('bash', ['-c', channelValidation.run], {
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
    assert.equal(logged, '', 'channel validation must not dispatch another workflow');
  }
} finally {
  rmSync(directory, { recursive: true, force: true });
}
console.log('release-publication-contract.test.mjs passed');
