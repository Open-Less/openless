import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  ANDROID_ABI_MATRIX,
  parseAndroidAbis,
  toGithubMatrixInclude,
  entryForAbi,
} from './android-abi-matrix.mjs';
import { collectSplitApks } from './collect-android-split-apks.mjs';

const scriptDir = fileURLToPath(new URL('.', import.meta.url));
const workflowPath = fileURLToPath(
  new URL('../../../.github/workflows/android-apk.yml', import.meta.url),
);

// --- ABI matrix ---
assert.equal(
  parseAndroidAbis('')
    .map((e) => e.abi)
    .join(','),
  'aarch64',
);
assert.equal(parseAndroidAbis('all').length, 4);
assert.equal(
  parseAndroidAbis('aarch64,armv7')
    .map((e) => e.abi)
    .join(','),
  'aarch64,armv7',
);
assert.equal(
  parseAndroidAbis('aarch64 aarch64,armv7')
    .map((e) => e.abi)
    .join(','),
  'aarch64,armv7',
);
assert.throws(() => parseAndroidAbis('riscv'), /Unknown Android ABI/);
assert.equal(entryForAbi('x86_64').gradleAbi, 'x86_64');
assert.equal(toGithubMatrixInclude(ANDROID_ABI_MATRIX)[0].rust_target, 'aarch64-linux-android');

const cli = spawnSync(
  process.execPath,
  [join(scriptDir, 'android-abi-matrix.mjs'), 'parse', 'aarch64'],
  { encoding: 'utf8' },
);
assert.equal(cli.status, 0);
assert.equal(JSON.parse(cli.stdout)[0].gradle_abi, 'arm64-v8a');

// --- collectSplitApks with a minimal zip APK ---
function writeMinimalApk(path, ...abis) {
  const python = process.platform === 'win32' ? 'python' : 'python3';
  const result = spawnSync(
    python,
    [
      '-c',
      `
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], 'w') as apk:
    for abi in sys.argv[2:]:
        apk.writestr('lib/' + abi + '/libdummy.so', b'so')
`,
      path,
      ...abis,
    ],
    { encoding: 'utf8' },
  );
  assert.equal(result.status, 0, result.stderr);
}

const tmp = mkdtempSync(join(tmpdir(), 'openless-apk-collect-'));
try {
  const androidRoot = join(tmp, 'android');
  const outDir = join(tmp, 'out');
  const apkDir = join(androidRoot, 'app', 'build', 'outputs', 'apk', 'release');
  mkdirSync(apkDir, { recursive: true });
  const apk = join(apkDir, 'app-arm64-v8a-release.apk');
  writeMinimalApk(apk, 'arm64-v8a');
  const options = {
    mode: 'release',
    label: 'test',
    expectedGradleAbis: ['arm64-v8a'],
    androidRoot,
    outDir,
    version: '2.0.0-Beta.3+build.20260925',
  };
  const { outputs } = collectSplitApks(options);
  assert.match(
    outputs.arm64_v8a_path.replace(/\\/g, '/'),
    /OpenLess_2\.0\.0-Beta\.3\+build\.20260925_arm64-v8a\.apk$/,
  );
  assert.equal(outputs.arm64_v8a_arch, 'aarch64');
  writeMinimalApk(apk, 'arm64-v8a', 'unexpected-abi');
  assert.throws(() => collectSplitApks(options), /Unknown ABI/);
  writeMinimalApk(apk, 'arm64-v8a', 'x86');
  assert.throws(() => collectSplitApks(options), /expected exactly one ABI/);
  writeMinimalApk(apk, 'arm64-v8a');
  writeFileSync(apk, readFileSync(apk).subarray(0, -22));
  assert.throws(() => collectSplitApks(options), /Invalid APK ZIP/);
  writeMinimalApk(apk, 'arm64-v8a');
  const corrupt = readFileSync(apk);
  corrupt[30 + Buffer.byteLength('lib/arm64-v8a/libdummy.so')] ^= 0xff;
  writeFileSync(apk, corrupt);
  assert.throws(() => collectSplitApks(options), /Invalid APK ZIP/);
  writeMinimalApk(apk, 'x86');
  assert.throws(() => collectSplitApks(options), /Missing split APKs/);
  writeMinimalApk(apk, 'arm64-v8a');
  writeMinimalApk(join(apkDir, 'duplicate.apk'), 'arm64-v8a');
  assert.throws(() => collectSplitApks(options), /Duplicate APKs/);
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

// --- workflow contract (#1103) ---
const workflow = readFileSync(workflowPath, 'utf8');
assert.match(workflow, /prefix-key:\s*v1-rust-android-1103/);
assert.doesNotMatch(workflow, /Free disk before artifact upload/);
assert.doesNotMatch(workflow, /rm -rf src-tauri\/target/);
assert.doesNotMatch(workflow, /rm -rf ~\/\.cargo\/registry/);
assert.doesNotMatch(workflow, /rm -rf ~\/\.gradle\/caches/);
assert.match(workflow, /abis:/);
assert.match(workflow, /default:\s*['"]aarch64['"]/);
assert.match(workflow, /fast_profile:/);
assert.match(workflow, /signed_debug:/);
assert.match(workflow, /signing=signed-debug/);
assert.match(workflow, /mode == 'release' \|\| needs\.plan\.outputs\.signing == 'signed-debug'/);
assert.match(workflow, /strategy:[\s\S]*matrix:/);
assert.match(workflow, /OPENLESS_ANDROID_TARGETS/);
assert.match(workflow, /CARGO_PROFILE_RELEASE_LTO/);
assert.match(workflow, /first ABI may compile twice|android-studio-script/);
assert.match(workflow, /publish-android-release/);
assert.match(workflow, /download-artifact/);
assert.match(workflow, /Rust cache/);

console.log('android-apk-workflow-contract checks passed');

