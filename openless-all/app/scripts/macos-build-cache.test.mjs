import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'darwin') {
  console.log('macOS build cache tests skipped on other platforms');
  process.exit(0);
}

const scripts = dirname(fileURLToPath(import.meta.url));
const root = mkdtempSync(join(tmpdir(), 'openless-macos-build-'));
const app = join(root, 'src-tauri/target/release/bundle/macos/OpenLess.app');
const dmg = join(root, 'src-tauri/target/release/bundle/dmg/OpenLess_1.2.3_x64.dmg');
const builds = join(root, 'src-tauri/target/release/build');

function put(path, content) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}

function executable(name, content) {
  const path = join(root, 'bin', name);
  put(path, `#!/usr/bin/env bash\nset -euo pipefail\n${content}\n`);
  chmodSync(path, 0o755);
}

function run(mode = 'success') {
  const env = { ...process.env };
  for (const name of Object.keys(env)) {
    if (/^(APPLE_|TAURI_SIGNING_|CARGO_PROFILE_RELEASE_|GITHUB_ENV$)/.test(name)) delete env[name];
  }
  return spawnSync('bash', ['scripts/build-mac.sh'], {
    cwd: root,
    encoding: 'utf8',
    env: { ...env, INSTALL: '0', BUILD_FIXTURE_MODE: mode, PATH: `${root}/bin:${env.PATH}` },
  });
}

try {
  mkdirSync(join(root, 'scripts'), { recursive: true });
  for (const name of [
    'build-mac.sh',
    'macos-build-env.sh',
    'check-macos-speech-usage-description.sh',
  ]) {
    copyFileSync(join(scripts, name), join(root, 'scripts', name));
  }
  put(join(root, 'package.json'), '{"version":"1.2.3"}');
  put(
    join(root, 'fixture.plist'),
    `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>NSMicrophoneUsageDescription</key><string>Microphone</string>
<key>NSSpeechRecognitionUsageDescription</key><string>Speech</string>
</dict></plist>`,
  );

  executable('uname', 'echo x86_64');
  executable('codesign', 'echo com.apple.security.device.audio-input');
  executable('xattr', 'exit 1');
  executable(
    'npm',
    `
if [ "$*" = "run check:macos-metal-toolchain" ]; then exit 0; fi
[[ "$*" == *"-- --locked --timings"* ]]
[[ "$CARGO_PROFILE_RELEASE_CODEGEN_UNITS" == 16 ]]
[[ "$CARGO_PROFILE_RELEASE_STRIP" == debuginfo ]]
app=src-tauri/target/release/bundle/macos/OpenLess.app
dmg=src-tauri/target/release/bundle/dmg/OpenLess_1.2.3_x64.dmg
# Old bundles must be gone before compilation starts, while Cargo stays warm.
[[ ! -e "$app" && ! -e "$dmg" && ! -e "$app.tar.gz" && ! -e "$app.tar.gz.sig" ]]
[[ -f src-tauri/target/release/build/qwen3-asr-rs-helper/build-script-build ]]
[[ -f src-tauri/target/release/build/qwen3-asr-rs-new/out/lib/mlx.metallib ]]
[[ ! -e src-tauri/target/release/build/qwen3-asr-rs-old ]]
if [ "$BUILD_FIXTURE_MODE" = missing ]; then exit 0; fi
mkdir -p "$app/Contents/MacOS" "$(dirname "$dmg")"
cp fixture.plist "$app/Contents/Info.plist"
echo binary > "$app/Contents/MacOS/openless"
# Cargo may reuse a binary compiled before this packaging invocation.
touch -t 200001010000 "$app/Contents/MacOS/openless"
echo dmg > "$dmg"
if [ "$BUILD_FIXTURE_MODE" = failure ]; then exit 23; fi
`,
  );

  put(join(builds, 'qwen3-asr-rs-helper/build-script-build'), 'cached executable');
  put(join(builds, 'qwen3-asr-rs-new/out/lib/mlx.metallib'), 'new shader');
  put(join(builds, 'qwen3-asr-rs-old/out/lib/mlx.metallib'), 'old shader');
  utimesSync(join(builds, 'qwen3-asr-rs-old/out/lib/mlx.metallib'), 1, 1);
  put(join(app, 'Contents/MacOS/openless'), 'stale binary');
  put(dmg, 'stale dmg');
  put(`${app}.tar.gz`, 'stale updater');
  put(`${app}.tar.gz.sig`, 'stale signature');

  for (let repeat = 0; repeat < 2; repeat += 1) {
    const result = run();
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(readFileSync(dmg, 'utf8').trim(), 'dmg');
    assert.equal(
      readFileSync(join(builds, 'qwen3-asr-rs-helper/build-script-build'), 'utf8'),
      'cached executable',
    );
  }
  const failure = run('failure');
  assert.equal(failure.status, 23, failure.stdout + failure.stderr);
  const missing = run('missing');
  assert.notEqual(missing.status, 0, missing.stdout + missing.stderr);
  assert.equal(existsSync(app), false);
  assert.equal(existsSync(dmg), false);
} finally {
  rmSync(root, { recursive: true, force: true });
}

console.log('macOS warm build and packaging failure tests passed');
