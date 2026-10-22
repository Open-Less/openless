// Contract test: release builds must restore the cache the baseline branch
// maintains, instead of compiling every dependency from a cold tree.
//
// `swatinem/rust-cache` builds its key as
//   <prefix-key>-<shared-key>-<os>-<arch>-<envHash>-<lockHash>
// with `restore-keys` falling back to everything before the hashes. The job name
// only enters the key when `shared-key` is absent, so two different jobs share a
// cache exactly when they declare the same `shared-key`.
//
// Runs without `npm ci` (the Linux contract job has no node_modules), so the
// workflows are read as text.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const read = (name) => readFileSync(join(root, '.github/workflows', name), 'utf8');

const ci = read('ci.yml');
const release = read('release-tauri.yml');
const androidApk = read('android-apk.yml');
const linux = read('check-linux-egui.yml');
const linuxRelease = read('release-linux-egui.yml');
const releaseHarmony = read('release-harmony.yml');

const sharedKeys = (yaml) =>
  [...yaml.matchAll(/shared-key:\s*([^\s#]+)/g)].map((match) => match[1].trim());

// Every write path must be gated on the beta branch. An ungated `actions/cache`
// writes one copy per pull request scope (`refs/pull/<n>/merge`), and those
// copies are never restored again - they just occupy the 10 GB budget and evict
// the branch trees. The HarmonyOS toolchain cache (1.77 GB) and the Rime NDK
// cache (400 MB) both leaked this way before this contract existed.
//
// `actions/cache@v4` saves on every run it executes, including pull request
// scopes (`refs/pull/<n>/merge`) that are never restored again. The rule is:
// restore unconditionally, save only from beta.
for (const [yaml, label] of [
  [ci, 'ci.yml'],
  [release, 'release-tauri.yml'],
  [androidApk, 'android-apk.yml'],
  [linux, 'check-linux-egui.yml'],
  [linuxRelease, 'release-linux-egui.yml'],
  [releaseHarmony, 'release-harmony.yml'],
]) {
  assert.ok(
    !/uses: actions\/cache@v4/.test(yaml),
    `${label} must not use actions/cache@v4 (auto-save); pair cache/restore@v4 with a beta-gated cache/save@v4`,
  );
}
// The extracted toolchain is 1.77 GB for a 2.1 GB download, so neither workflow
// may cache it.
for (const [yaml, label] of [
  [ci, 'ci.yml'],
  [releaseHarmony, 'release-harmony.yml'],
]) {
  assert.ok(
    !yaml.includes('harmony-tools-'),
    `${label} must not cache the HarmonyOS toolchain: the download is shorter than restoring 1.77 GB`,
  );
}

// 1. The baseline job must own a stable, job-name-independent key per platform.
for (const key of ['tauri-windows', 'tauri-macos']) {
  assert.ok(
    sharedKeys(ci).includes(key),
    `ci.yml must declare shared-key ${key} so the release workflow can reuse it`,
  );
  assert.ok(sharedKeys(release).includes(key), `release-tauri.yml must restore shared-key ${key}`);
}

// 2. The release workflow must not keep a private key: that is what made tag
//    builds cold (the baseline branch never wrote it).
assert.doesNotMatch(release, /key:\s*macos-release/);

// 3. Baseline writes stay gated to the default branch, releases only read it.
const saveIfs = [
  ...ci.matchAll(/save-if:\s*\$\{\{\s*github\.ref == 'refs\/heads\/beta'[^}]*\}\}/g),
];
assert.ok(saveIfs.length >= 3, 'every shared cache in ci.yml must be beta-only for writes');

// 4. The APK build keeps its per-ABI key (it builds a different target triple
//    than `android-check`), but must not define a private ad-hoc `key`.
assert.doesNotMatch(androidApk, /^\s*key:\s/m);
assert.ok(sharedKeys(androidApk).some((key) => key.startsWith('android-')));

// 5. Linux already shares one job between CI and release: the release entry point
//    must reuse the same reusable workflow rather than a second build path.
assert.match(linuxRelease, /uses: \.\/\.github\/workflows\/check-linux-egui\.yml/);
assert.match(linux, /cargo build --locked --release -p openless-linux-egui/);

// 6. Release entry points pin the tag, so the artifacts cannot come from a branch.
assert.match(linuxRelease, /release_tag: \$\{\{ inputs\.tag \}\}/);
assert.match(linuxRelease, /checkout_ref: \$\{\{ inputs\.tag \}\}/);

// The capsule crates used to run in their own pull-request-only workflow: it
// wrote a dependency tree under `refs/pull/<n>/merge` (never restored again)
// while beta produced nothing to restore. It now lives here, behind the same
// `Changed areas` verdict, and saves only from beta.
{
  const capsuleJob = ci.match(/\n  native-capsule:[\s\S]*?(?=\n  [a-z-]+:\n)/)?.[0] ?? '';
  assert.ok(capsuleJob, 'ci.yml must run the capsule crates as the native-capsule job');
  assert.match(capsuleJob, /shared-key: native-capsule/);
  assert.match(capsuleJob, /save-if: \$\{\{ github\.ref == 'refs\/heads\/beta'/);
  assert.ok(
    !existsSync(join(root, '.github/workflows/native-capsule.yml')),
    'native-capsule.yml must stay merged into ci.yml',
  );
}

const linuxJob = (name) =>
  linux.match(new RegExp('\\n  ' + name + ':[\\s\\S]*?(?=\\n  [a-z-]+:|$)'))?.[0] ?? '';
assert.match(linuxJob('test-linux-egui'), /shared-key: test-linux-egui/);
assert.match(linuxJob('build-linux-egui'), /shared-key: build-linux-egui/);
const before = (producer, save) => {
  const producerIndex = ci.indexOf('- name: ' + producer);
  const saveIndex = ci.indexOf('- name: ' + save);
  assert.ok(
    producerIndex >= 0 && producerIndex < saveIndex,
    save + ' must follow its successful producer',
  );
};
before(
  'Build and stage Rime for Android instrumentation tests',
  'Save Android Rime dependencies on beta builds',
);
before(
  'Prepare the pinned Android emulator and system image',
  'Save the Android emulator on beta builds',
);
assert.doesNotMatch(
  ci,
  /reactivecircus\/android-emulator-runner|~\/\.android\/avd|android-emulator-37\.2\.12/,
);
assert.match(ci, /android-emulator-v2-16555280-api35-google_apis-x86_64-/);
for (const id of ['rime_cache', 'emulator_cache']) {
  assert.match(
    ci,
    new RegExp(
      'if: steps\\.' +
        id +
        "\\.outputs\\.cache-hit != 'true' && github\\.ref == 'refs/heads/beta' && \\(github\\.event_name == 'push' \\|\\| github\\.event_name == 'workflow_dispatch'\\)",
    ),
  );
}
console.log('ci-cache-sharing contract test passed');
