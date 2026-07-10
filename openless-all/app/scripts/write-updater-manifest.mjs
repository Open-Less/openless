#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const target = process.env.OPENLESS_UPDATE_TARGET;
const arch = process.env.OPENLESS_UPDATE_ARCH;
const repo = process.env.OPENLESS_UPDATE_REPO || 'appergb/openless';
const mirrorBaseUrl = process.env.OPENLESS_UPDATE_MIRROR_BASE_URL || 'https://fastgit.cc/https://github.com';
const rawChannel = (process.env.OPENLESS_RELEASE_CHANNEL || 'stable').toLowerCase();
if (rawChannel !== 'stable' && rawChannel !== 'beta') {
  throw new Error(`Invalid OPENLESS_RELEASE_CHANNEL: "${rawChannel}" (expected "stable" or "beta")`);
}
const channelSuffix = rawChannel === 'beta' ? '-beta' : '';
const releaseTag = process.env.OPENLESS_RELEASE_TAG || '';
if (rawChannel === 'beta' && !releaseTag) {
  throw new Error('OPENLESS_RELEASE_TAG is required when OPENLESS_RELEASE_CHANNEL=beta');
}

if (!target || !arch) {
  throw new Error('OPENLESS_UPDATE_TARGET and OPENLESS_UPDATE_ARCH are required');
}

const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));

/** Map Gradle ABI folder names to updater manifest arch (Tauri {{arch}} convention). */
const ANDROID_ABI_TO_ARCH = {
  'arm64-v8a': 'aarch64',
  'armeabi-v7a': 'armv7',
  x86: 'i686',
  x86_64: 'x86_64',
};

function resolveBundleDir() {
  if (target === 'android') {
    const apkDir = process.env.OPENLESS_UPDATE_APK_DIR;
    if (!apkDir) {
      throw new Error('OPENLESS_UPDATE_APK_DIR is required when OPENLESS_UPDATE_TARGET=android');
    }
    return apkDir;
  }
  return fileURLToPath(new URL('../src-tauri/target/release/bundle/', import.meta.url));
}

const bundleDir = resolveBundleDir();

const candidatesByTarget = {
  darwin: [
    `macos/OpenLess_${arch}.app.tar.gz`,
    'macos/OpenLess.app.tar.gz',
  ],
  windows: ['nsis/OpenLess_*_x64-setup.exe', 'nsis/OpenLess*_x64-setup.exe'],
  linux: ['appimage/OpenLess_*.AppImage', 'appimage/OpenLess*.AppImage'],
  android: [],
};

function findAndroidApk() {
  const version = packageJson.version;
  const abiEntry = Object.entries(ANDROID_ABI_TO_ARCH).find(([, manifestArch]) => manifestArch === arch);
  if (!abiEntry) {
    throw new Error(`Unknown android updater arch: ${arch}`);
  }
  const [abi] = abiEntry;
  const expectedName = `OpenLess_${version}_${abi}.apk`;
  const direct = join(bundleDir, expectedName);
  if (existsSync(direct)) {
    return direct;
  }
  const match = readdirSync(bundleDir)
    .filter((name) => name.endsWith('.apk') && name.includes(abi))
    .sort()[0];
  if (match) {
    return join(bundleDir, match);
  }
  throw new Error(`No Android APK found for arch ${arch} (abi ${abi}) in ${bundleDir}`);
}

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

const artifact = target === 'android' ? findAndroidApk() : findFirst(candidatesByTarget[target] || []);
if (!artifact) {
  throw new Error(`No updater artifact found for ${target} in ${bundleDir}`);
}

const signaturePath = `${artifact}.sig`;
if (!existsSync(signaturePath)) {
  throw new Error(`Missing updater signature: ${signaturePath}`);
}

const assetName = basename(artifact);
const manifestName = `latest-${target}-${arch}${channelSuffix}.json`;
const mirrorManifestName = `latest-${target}-${arch}${channelSuffix}-mirror.json`;
const downloadPath = rawChannel === 'beta'
  ? `releases/download/${releaseTag}/${assetName}`
  : `releases/latest/download/${assetName}`;
const githubAssetUrl = `https://github.com/${repo}/${downloadPath}`;
const mirrorAssetUrl = `${mirrorBaseUrl.replace(/\/$/, '')}/${repo}/${downloadPath}`;
const manifest = {
  version: packageJson.version,
  pub_date: new Date().toISOString(),
  url: githubAssetUrl,
  signature: readFileSync(signaturePath, 'utf8').trim(),
};
const mirrorManifest = {
  ...manifest,
  url: mirrorAssetUrl,
};

const outDir = target === 'android' ? bundleDir : bundleDir;
writeFileSync(join(outDir, manifestName), `${JSON.stringify(manifest, null, 2)}\n`);
writeFileSync(join(outDir, mirrorManifestName), `${JSON.stringify(mirrorManifest, null, 2)}\n`);
console.log(`Wrote ${manifestName} and ${mirrorManifestName} for ${assetName}`);
