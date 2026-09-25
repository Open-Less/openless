import { readdir, readFile } from 'node:fs/promises';
import { access, constants } from 'node:fs/promises';
import { basename, extname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// The Tauri shell no longer ships a Linux desktop target: Linux runs the egui
// host in `openless-all/app/linux-egui`. This contract keeps that split from
// drifting back — a Linux-only cfg block, a Linux-only dependency, or a Linux
// branch in the React frontend would silently re-grow an unsupported target.
//
// Kept on purpose (so the checks below stay narrow):
//   - `src-tauri/src/lib.rs` keeps a `#[cfg(target_os = "linux")] compile_error!`
//     guard so an accidental Linux build fails with an actionable message.
//   - i18n platform descriptions may legitimately say "Windows / Linux"
//     because Linux users still run the product through the egui host.

const appRoot = new URL('..', import.meta.url);
const tauriSrc = fileURLToPath(new URL('../src-tauri/src', import.meta.url));
const reactSrc = fileURLToPath(new URL('../src', import.meta.url));
const failures = [];

async function collect(dir, extensions, { skip = () => false } = {}) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (entry.name === 'target' || entry.name === 'node_modules') continue;
    const child = join(dir, entry.name);
    if (skip(child)) continue;
    if (entry.isDirectory()) files.push(...await collect(child, extensions, { skip }));
    else if (extensions.has(extname(entry.name))) files.push(child);
  }
  return files;
}

// ── 1. Rust: no Linux cfg branches beyond the compile_error! guard ──────────
const rustFiles = await collect(tauriSrc, new Set(['.rs']));
for (const file of rustFiles) {
  const source = await readFile(file, 'utf8');
  const lines = source.split('\n');
  lines.forEach((line, index) => {
    const normalized = line.trim();
    if (!normalized.includes('target_os = "linux"')) return;
    const isGuard = basename(file) === 'lib.rs'
      && /^#\[cfg\(target_os = "linux"\)\]$/.test(normalized)
      && lines.slice(index, index + 3).some((entry) => entry.includes('compile_error!'));
    if (!isGuard) failures.push(`${file}:${index + 1}: Linux cfg branch`);
  });
  if (/#!?\[cfg_attr\(target_os = "linux"/.test(source)) {
    failures.push(`${file}: still suppresses dead code with a Linux cfg_attr`);
  }
}

// ── 2. Cargo manifest: no Linux target deps, no Linux-only patch ─────────────
const manifest = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
if (/\[target\..*cfg\(target_os = "linux"\)/.test(manifest)) {
  failures.push('src-tauri/Cargo.toml: declares a Linux target dependency table');
}
for (const forbidden of ['wayland-scanner', 'linux-native-sync-persistent', '[patch.crates-io]']) {
  if (manifest.includes(forbidden)) {
    failures.push(`src-tauri/Cargo.toml: still references ${forbidden}`);
  }
}
try {
  await access(new URL('../src-tauri/vendor/wayland-scanner', import.meta.url), constants.F_OK);
  failures.push('src-tauri/vendor/wayland-scanner: Linux-only vendored patch is still present');
} catch {
  // absent, as expected
}

// ── 3. React: no Linux branch (i18n copy is excluded, see the header) ────────
const reactFiles = await collect(reactSrc, new Set(['.ts', '.tsx']), {
  skip: (path) => /[/\\]i18n[/\\]/.test(path),
});
for (const file of reactFiles) {
  if (file.endsWith('.test.ts') || file.endsWith('.test.tsx')) continue;
  const source = await readFile(file, 'utf8');
  for (const pattern of [/LINUX_TITLEBAR_HEIGHT/, /ol-linux-/, /['"]linux['"]/]) {
    const match = source.match(pattern);
    if (match) failures.push(`${file}: React still branches on Linux (${match[0]})`);
  }
}

// ── 4. The guard itself must stay meaningful ────────────────────────────────
const libSource = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
if (!/compile_error!\(\s*"Tauri 版已不再支持 Linux 桌面/.test(libSource)) {
  failures.push('src-tauri/src/lib.rs: the Linux compile_error! guard is missing');
}

if (failures.length) {
  throw new Error(`Tauri Linux-free contract failed:\n${failures.join('\n')}`);
}

console.log(
  `tauri-linux-free-contract.test.mjs passed (${rustFiles.length} rust files, ${reactFiles.length} react files)`,
);
