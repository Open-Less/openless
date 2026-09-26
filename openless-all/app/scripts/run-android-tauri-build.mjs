/**
 * Run `tauri android build` with ABI targets from OPENLESS_ANDROID_TARGETS
 * (space/comma-separated CLI names: aarch64 armv7 i686 x86_64).
 *
 * Usage:
 *   node scripts/run-android-tauri-build.mjs --debug
 *   node scripts/run-android-tauri-build.mjs --release
 */
import { spawnSync } from 'node:child_process';
import process from 'node:process';
import { parseAndroidAbis, ANDROID_ABI_MATRIX } from './android-abi-matrix.mjs';

function resolveTargets() {
  const raw = process.env.OPENLESS_ANDROID_TARGETS ?? '';
  if (!raw.trim()) {
    return ANDROID_ABI_MATRIX.map((e) => e.abi);
  }
  return parseAndroidAbis(raw, {
    defaultAbis: ANDROID_ABI_MATRIX.map((e) => e.abi),
  }).map((e) => e.abi);
}

function main() {
  const mode = process.argv[2];
  if (mode !== '--debug' && mode !== '--release') {
    console.error('Usage: node scripts/run-android-tauri-build.mjs --debug|--release');
    process.exit(1);
  }
  const targets = resolveTargets();
  const args = ['tauri', 'android', 'build', '--apk', '--split-per-abi', '--target', ...targets];
  if (mode === '--debug') {
    args.push('--debug');
  }

  console.log(`[android-build] targets=${targets.join(',')} mode=${mode.slice(2)}`);
  // #1103: Tauri CLI runs an initial cargo build for the first target to
  // "initialize plugins", then Gradle invokes `android-studio-script` per ABI
  // (including the first). Expect two cargo passes for the first ABI; do not
  // bypass the Gradle/native path.
  console.log(
    '[android-build] note: first ABI may compile twice (Tauri plugin init + android-studio-script)',
  );

  const bin = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  const result = spawnSync(bin, args, {
    stdio: 'inherit',
    env: process.env,
    shell: process.platform === 'win32',
  });
  if (result.error) {
    throw result.error;
  }
  process.exit(result.status ?? 1);
}

main();
