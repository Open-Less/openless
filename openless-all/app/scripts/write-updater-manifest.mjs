#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const target = process.env.OPENLESS_UPDATE_TARGET;
const arch = process.env.OPENLESS_UPDATE_ARCH;
const repo = process.env.OPENLESS_UPDATE_REPO || 'appergb/openless';

if (!target || !arch) {
  throw new Error('OPENLESS_UPDATE_TARGET and OPENLESS_UPDATE_ARCH are required');
}

const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const bundleDir = fileURLToPath(new URL('../src-tauri/target/release/bundle/', import.meta.url));

const candidatesByTarget = {
  darwin: ['macos/OpenLess.app.tar.gz'],
  windows: ['nsis/OpenLess_*_x64-setup.exe', 'nsis/OpenLess*_x64-setup.exe'],
  linux: ['appimage/OpenLess_*.AppImage', 'appimage/OpenLess*.AppImage'],
};

function findFirst(patterns) {
  for (const pattern of patterns) {
    if (!pattern.includes('*')) {
      const path = join(bundleDir, pattern);
      if (existsSync(path)) return path;
      continue;
    }
    const [dir, namePattern] = pattern.split('/');
    const dirPath = join(bundleDir, dir);
    if (!existsSync(dirPath)) continue;
    const prefix = namePattern.split('*')[0];
    const suffix = namePattern.split('*').at(-1);
    const match = readdirSync(dirPath)
      .filter(name => name.startsWith(prefix) && name.endsWith(suffix))
      .sort()[0];
    if (match) return join(dirPath, match);
  }
}

const artifact = findFirst(candidatesByTarget[target] || []);
if (!artifact) {
  throw new Error(`No updater artifact found for ${target} in ${bundleDir}`);
}

const signaturePath = `${artifact}.sig`;
if (!existsSync(signaturePath)) {
  throw new Error(`Missing updater signature: ${signaturePath}`);
}

const assetName = basename(artifact);
const manifestName = `latest-${target}-${arch}.json`;
const manifest = {
  version: packageJson.version,
  pub_date: new Date().toISOString(),
  url: `https://github.com/${repo}/releases/latest/download/${assetName}`,
  signature: readFileSync(signaturePath, 'utf8').trim(),
};

writeFileSync(join(bundleDir, manifestName), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Wrote ${manifestName} for ${assetName}`);
