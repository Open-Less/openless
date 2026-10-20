import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(new URL('./stage-release-download-aliases.mjs', import.meta.url));

function run(options) {
  const result = spawnSync(process.execPath, [scriptPath], {
    cwd: fileURLToPath(new URL('..', import.meta.url)),
    env: {
      ...process.env,
      OPENLESS_DOWNLOAD_TARGET: options.target,
      OPENLESS_DOWNLOAD_ARCH: options.arch,
      OPENLESS_DOWNLOAD_SOURCE_DIR: options.sourceDir,
      OPENLESS_DOWNLOAD_OUTPUT_DIR: options.outputDir,
    },
    encoding: 'utf8',
  });
  return result;
}

const root = mkdtempSync(join(tmpdir(), 'openless-download-aliases-'));
try {
  const darwinSource = join(root, 'darwin-source');
  const darwinOutput = join(root, 'darwin-output');
  mkdirSync(darwinSource);
  writeFileSync(join(darwinSource, 'OpenLess_2.0.0_aarch64.dmg'), 'arm64 dmg');
  let result = run({
    target: 'darwin',
    arch: 'aarch64',
    sourceDir: darwinSource,
    outputDir: darwinOutput,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(darwinOutput, 'OpenLess_macos_aarch64.dmg'), 'utf8'), 'arm64 dmg');

  const darwinIntelSource = join(root, 'darwin-intel-source');
  const darwinIntelOutput = join(root, 'darwin-intel-output');
  mkdirSync(darwinIntelSource);
  writeFileSync(join(darwinIntelSource, 'OpenLess_2.0.0_x64.dmg'), 'x64 dmg');
  result = run({
    target: 'darwin',
    arch: 'x86_64',
    sourceDir: darwinIntelSource,
    outputDir: darwinIntelOutput,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    readFileSync(join(darwinIntelOutput, 'OpenLess_macos_x86_64.dmg'), 'utf8'),
    'x64 dmg',
  );

  const windowsSource = join(root, 'windows-source');
  const windowsOutput = join(root, 'windows-output');
  mkdirSync(windowsSource);
  writeFileSync(join(windowsSource, 'OpenLess_2.0.0_x64-setup.exe'), 'windows exe');
  writeFileSync(join(windowsSource, 'OpenLess_2.0.0_x64-setup.exe.sig'), 'signature');
  result = run({
    target: 'windows',
    arch: 'x86_64',
    sourceDir: windowsSource,
    outputDir: windowsOutput,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    readFileSync(join(windowsOutput, 'OpenLess_windows_x86_64-setup.exe'), 'utf8'),
    'windows exe',
  );
  assert.equal(
    readFileSync(join(windowsOutput, 'OpenLess_windows_x86_64-setup.exe.sig'), 'utf8'),
    'signature',
  );

  const androidSource = join(root, 'android-source');
  const androidOutput = join(root, 'android-output');
  mkdirSync(androidSource);
  writeFileSync(join(androidSource, 'OpenLess_2.0.0_arm64-v8a.apk'), 'android apk');
  result = run({
    target: 'android',
    arch: 'aarch64',
    sourceDir: androidSource,
    outputDir: androidOutput,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    readFileSync(join(androidOutput, 'OpenLess_android_arm64-v8a.apk'), 'utf8'),
    'android apk',
  );

  const duplicateSource = join(root, 'duplicate-source');
  mkdirSync(duplicateSource);
  writeFileSync(join(duplicateSource, 'OpenLess_1_aarch64.dmg'), 'one');
  writeFileSync(join(duplicateSource, 'OpenLess_2_aarch64.dmg'), 'two');
  result = run({
    target: 'darwin',
    arch: 'aarch64',
    sourceDir: duplicateSource,
    outputDir: join(root, 'duplicate-output'),
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Expected exactly one macOS aarch64 DMG/);
} finally {
  rmSync(root, { recursive: true, force: true });
}

console.log('stage-release-download-aliases checks passed');
