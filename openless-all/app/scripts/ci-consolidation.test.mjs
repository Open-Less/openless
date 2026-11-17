import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { load } from 'js-yaml';

const path = (name) => new URL(`../../../.github/workflows/${name}`, import.meta.url);
const workflow = (name) => load(readFileSync(path(name), 'utf8'));
const ci = workflow('ci.yml');
assert.equal(existsSync(path('check-ios.yml')), false, 'iOS must not run twice');
assert.equal(ci.jobs.changes.outputs.ios, '${{ steps.detect.outputs.ios-device }}');
assert.match(
  ci.jobs.changes.steps.find((s) => s.id === 'detect').run,
  /for area in .* ios-device; do/,
);
for (const id of ['ios-test', 'ios-device-build']) {
  const job = ci.jobs[id];
  assert.ok(job.needs.includes('changes'));
  assert.equal(job['runs-on'], 'xcode-27');
  assert.equal(job.env.CARGO_PROFILE_DEV_DEBUG, 0);
  assert.equal(job.env.CARGO_PROFILE_TEST_DEBUG, 0);
  assert.equal(
    job.if,
    "(github.event_name != 'workflow_dispatch' || inputs.platform != 'macos') && needs.changes.outputs.ios != 'false'",
  );
  assert.ok(job.steps.some((s) => s.run?.includes('git diff --exit-code')));
}
assert.equal(ci.jobs['ios-test'].name, 'test', 'preserve the existing required check name');
assert.equal(ci.jobs['ios-device-build'].name, 'device-build');
const test = ci.jobs['ios-test'];
const testCommands = test.steps.map((s) => s.run ?? '').join('\n');
for (const command of [
  'prepare-core.py',
  'swift test',
  'cargo test',
  'cargo clippy',
  'test-interface-resources.py',
  'build-predictions.py',
  'test-predictions.sh',
  'test-audio.sh',
]) {
  assert.ok(testCommands.includes(command), `retain iOS check: ${command}`);
}
const device = ci.jobs['ios-device-build'];
assert.deepEqual(
  device.needs,
  ['changes'],
  'independent iOS checks should reuse the same two VMs in parallel',
);
const deviceCommands = device.steps.map((s) => s.run ?? '').join('\n');
for (const [job, family, workspace, timingPath] of [
  [
    test,
    'ios-host',
    'openless-all/ios/CoreHost -> target',
    'openless-all/ios/CoreHost/target/cargo-timings/',
  ],
  [
    device,
    'ios-device',
    'openless-all/ios/CoreHost -> ../.build/core-ios16',
    'openless-all/ios/.build/core-ios16/cargo-timings/',
  ],
]) {
  const cache = job.steps.find((s) => s.uses === 'swatinem/rust-cache@v2');
  assert.equal(cache.with['shared-key'], family);
  assert.equal(cache.with.workspaces, workspace);
  assert.ok(cache.with.key.includes('steps.xcode.outputs.version'));
  assert.match(
    cache.with['save-if'],
    /github\.ref == 'refs\/heads\/beta'.*github\.event_name == 'push'.*workflow_dispatch/,
  );
  const timing = job.steps.find((s) => s.with?.name === `${family}-cargo-timings`);
  assert.equal(timing.with.path, timingPath);
  assert.equal(timing.with['retention-days'], 7);
  assert.equal(timing.if, '${{ !cancelled() }}');
}
assert.ok(
  device.steps.find((s) => s.uses === 'swatinem/rust-cache@v2').with.key.startsWith('ios16-'),
);
for (const name of [
  'Test shared keyboard contracts',
  'Test mobile Core and iOS Host',
  'Check mobile Core',
  'Test interface resources, predictions and native audio',
]) {
  assert.ok(test.steps.find((s) => s.name === name)['timeout-minutes'] > 0);
}
for (const name of [
  'Prepare native build tools and dependencies',
  'Prepare verified local speech dependencies',
  'Build Rime and test composition',
  'Build the mobile Core and refresh attribution',
  'Build and validate the unsigned iPhone app',
]) {
  assert.ok(device.steps.find((s) => s.name === name)['timeout-minutes'] > 0);
}
assert.ok(testCommands.includes('--locked --timings'));
for (const command of [
  'bootstrap.sh',
  'bootstrap-local-speech.sh',
  'build-rime.sh host',
  'test-rime.sh',
  'build-rime.sh device',
  'build-core.sh device',
  'collect-rust-licenses.py',
  'validate-project.py',
  'build.sh device Release unsigned',
  'package-ipa.py',
]) {
  assert.ok(deviceCommands.includes(command), `retain device check: ${command}`);
}
assert.equal(ci.jobs['linux-egui-package'], undefined, 'Linux CI is paused');
for (const name of ['check-linux-egui.yml', 'release-linux-egui.yml']) {
  assert.equal(
    existsSync(path(name)),
    false,
    `${name} must stay removed while Linux automation is paused`,
  );
}
assert.equal(ci.jobs.changes.outputs.linux, undefined);
assert.equal(
  ci.jobs['windows-frontend'],
  undefined,
  'do not launch a duplicate Windows frontend VM',
);
const stable = ci.jobs['cross-platform'];
const windowsContracts = stable.steps.find(
  (s) => s.name === 'Run the platform-sensitive contracts',
);
assert.equal(windowsContracts.if, "runner.os == 'Windows'");
assert.equal(windowsContracts.run, 'node scripts/frontend-test-runner.mjs --platform-only windows');
const frontend = stable.steps.find((s) => s.name === 'Build the frontend for the Tauri context');
assert.equal(frontend.if, "runner.os == 'Windows'");
assert.ok(stable.steps.indexOf(frontend) < stable.steps.indexOf(windowsContracts));
assert.ok(
  stable.steps.indexOf(windowsContracts) <
    stable.steps.findIndex((s) => s.name === 'Compile Rust backend unit tests (Windows)'),
  'fail fast on contract errors',
);
assert.equal(
  stable.steps.find((s) => s.name === 'Check Windows prerequisites').if,
  'matrix.preflight',
);
assert.equal(stable.steps.filter((s) => s.name === 'Check PowerShell scripts').length, 1);
assert.equal(
  stable.steps.find((s) => s.name === 'Build frontend and run frontend/contract tests').run,
  'npm test',
);
assert.ok(
  ci.jobs['windows-checks'].needs.includes('cross-platform'),
  'the Windows gate must include frontend failures',
);
const cache = workflow('cache-maintenance.yml');
assert.equal(cache.permissions['pull-requests'], 'read');
assert.match(
  cache.jobs.report.steps[0].with.ref,
  /github\.event_name == 'pull_request'.*github\.event\.repository\.default_branch/,
);
assert.equal(cache.jobs.report.steps[0].with['persist-credentials'], false);
assert.match(
  cache.jobs.report.steps.find((s) => s.run).run,
  /--prune --keep-family 2 --prune-budget/,
);
const sync = workflow('sync-micyou.yml');
assert.equal(sync.jobs.candidate.permissions?.contents ?? sync.permissions.contents, 'read');
assert.equal(
  sync.jobs.candidate.steps.find((s) => s.id === 'sync').env.GH_TOKEN,
  '${{ github.token }}',
);
console.log('CI consolidation and maintenance contracts passed');
