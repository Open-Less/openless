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
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const read = (name) => readFileSync(join(root, '.github/workflows', name), 'utf8');

const ci = read('ci.yml');
const release = read('release-tauri.yml');
const androidApk = read('android-apk.yml');
const linux = read('check-linux-egui.yml');
const linuxRelease = read('release-linux-egui.yml');

const sharedKeys = (yaml) =>
  [...yaml.matchAll(/shared-key:\s*([^\s#]+)/g)].map((match) => match[1].trim());

// 1. The baseline job must own a stable, job-name-independent key per platform.
for (const key of ['tauri-windows', 'tauri-macos']) {
  assert.ok(
    sharedKeys(ci).includes(key),
    `ci.yml must declare shared-key ${key} so the release workflow can reuse it`,
  );
  assert.ok(
    sharedKeys(release).includes(key),
    `release-tauri.yml must restore shared-key ${key}`,
  );
}

// 2. The release workflow must not keep a private key: that is what made tag
//    builds cold (the baseline branch never wrote it).
assert.doesNotMatch(release, /key:\s*macos-release/);

// 3. Baseline writes stay gated to the default branch, releases only read it.
const saveIfs = [...ci.matchAll(/save-if:\s*\$\{\{\s*github\.ref == 'refs\/heads\/beta'[^}]*\}\}/g)];
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

console.log('ci-cache-sharing contract test passed');
