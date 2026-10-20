/**
 * Prepare native Android dependencies and run `tauri android dev`.
 *
 * The direct Tauri dev command does not know about the verified Sherpa
 * libraries or the mobile Rime staging step used by the release wrapper.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import {
  prepareLocalAsrLibs,
  resolveTargets,
  stageMobileRime,
} from './run-android-tauri-build.mjs';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const generatedAndroid = join(appRoot, 'src-tauri', 'gen', 'android');

function npxBin() {
  return process.platform === 'win32' ? 'npx.cmd' : 'npx';
}

function run(args, env) {
  const result = spawnSync(npxBin(), args, {
    stdio: 'inherit',
    env,
    shell: process.platform === 'win32',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${args.join(' ')} failed (exit ${result.status})`);
  }
}

function runNode(script, args, env) {
  const command = [script, ...args];
  const result = spawnSync(process.execPath, command, {
    stdio: 'inherit',
    env,
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command.join(' ')} failed (exit ${result.status})`);
  }
}

async function main() {
  const env = { ...process.env };
  const targets = resolveTargets();
  if (!existsSync(generatedAndroid)) {
    console.log('[android-dev] generated Android project missing; initializing it');
    run(['tauri', 'android', 'init', '--ci'], env);
  }
  // Keep direct development builds identical to the release wrapper: Tauri's
  // generated project is only a base, so copy the keyboard sources and apply
  // the manifest/Gradle integrations before starting Gradle.
  for (const script of [
    'copy-android-scaffolding.mjs',
    'merge-android-v1-manifest.mjs',
    'merge-android-overlay-manifest.mjs',
    'merge-android-shizuku-manifest.mjs',
    'patch-android-shizuku-deps.mjs',
    'merge-android-updater-manifest.mjs',
  ]) {
    runNode(join(appRoot, 'scripts', script), [], env);
  }
  await prepareLocalAsrLibs(targets, env);
  stageMobileRime(targets, env);
  console.log('[android-dev] starting Tauri Android development mode');
  run(['tauri', 'android', 'dev'], env);
}

main().catch((error) => {
  console.error(`[android-dev] ${error.message ?? error}`);
  process.exit(1);
});
