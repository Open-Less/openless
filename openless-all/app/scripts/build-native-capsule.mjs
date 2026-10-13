// Build a separate graphics executable: the Tauri host keeps its Rust 1.88
// dependency graph and never links egui/eframe. Native UI requires Rust 1.95+.
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const debug = process.argv.includes('--debug');
const profile = debug ? 'debug' : 'release';
const cargo = process.env.CARGO ?? 'cargo';
const rustc = process.env.RUSTC ?? 'rustc';
const info = spawnSync(rustc, ['-vV'], { encoding: 'utf8' });
if (info.status !== 0) throw new Error(info.stderr || 'rustc is unavailable');
const host = /^host: (.+)$/m.exec(info.stdout)?.[1];
const target = process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (!target || (!target.includes('darwin') && !target.includes('windows'))) {
  throw new Error(
    'The desktop capsule bundling step requires a macOS or Windows target. Linux uses its existing egui host.',
  );
}
const args = ['build', '--locked', '-p', 'openless-native-capsule', '--target', target];
if (!debug) args.push('--release');
const build = spawnSync(cargo, args, { cwd: root, stdio: 'inherit' });
if (build.error) throw build.error;
if (build.status !== 0) process.exit(build.status ?? 1);
const suffix = target.includes('windows') ? '.exe' : '';
const name = `openless-native-capsule${suffix}`;
const targetRoot = process.env.CARGO_TARGET_DIR
  ? resolve(root, process.env.CARGO_TARGET_DIR)
  : resolve(root, 'target');
const source = resolve(targetRoot, target, profile, name);
const binaries = resolve(root, 'src-tauri/binaries');
mkdirSync(binaries, { recursive: true });
// Tauri's externalBin convention uses a target-suffixed source filename.
copyFileSync(source, resolve(binaries, `openless-native-capsule-${target}${suffix}`));
// Raw cargo/tauri dev launches also discover the helper next to the host.
const hostOutput = resolve(root, 'src-tauri/target', profile);
mkdirSync(hostOutput, { recursive: true });
copyFileSync(source, resolve(hostOutput, name));
console.log(`[native-capsule] prepared ${target} ${profile} helper`);
