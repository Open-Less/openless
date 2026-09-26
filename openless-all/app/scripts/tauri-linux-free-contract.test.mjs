import { readFile, readdir } from 'node:fs/promises';
import { extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// Linux desktop ships as openless-linux-egui. This branch still keeps Linux
// cfg in the Tauri host because the shared desktop path (hotkeys, coordinator,
// adapters) was not split out. The React shell must not grow a second Linux UI.
// A dangling wayland-scanner patch is rejected: that vendor tree is not in the repo.

const appRoot = new URL('..', import.meta.url);
const reactSrc = fileURLToPath(new URL('../src', import.meta.url));
const failures = [];

async function collect(dir, extensions, { skip = () => false } = {}) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (entry.name === 'target' || entry.name === 'node_modules') continue;
    const child = join(dir, entry.name);
    if (skip(child)) continue;
    if (entry.isDirectory()) files.push(...(await collect(child, extensions, { skip })));
    else if (extensions.has(extname(entry.name))) files.push(child);
  }
  return files;
}

const manifest = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
if (manifest.includes('vendor/wayland-scanner')) {
  failures.push('src-tauri/Cargo.toml: patches a wayland-scanner tree that is not vendored');
}

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

if (failures.length) {
  throw new Error(`Tauri Linux-free contract failed:\n${failures.join('\n')}`);
}

console.log(`tauri-linux-free-contract.test.mjs passed (${reactFiles.length} react files)`);
