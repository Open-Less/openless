#!/usr/bin/env node
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import process from 'node:process';

const TARGETS = new Set(['darwin', 'windows', 'android']);
const ANDROID_ABIS = {
  aarch64: 'arm64-v8a',
  armv7: 'armeabi-v7a',
  i686: 'x86',
  x86_64: 'x86_64',
};
const DARWIN_BUNDLE_ARCHES = {
  aarch64: 'aarch64',
  x86_64: 'x64',
};

function readOption(name) {
  const index = process.argv.indexOf(`--${name}`);
  if (index !== -1) {
    const value = process.argv[index + 1];
    if (!value || value.startsWith('--')) {
      throw new Error(`Missing value for --${name}`);
    }
    return value;
  }
  return process.env[`OPENLESS_DOWNLOAD_${name.replaceAll('-', '_').toUpperCase()}`];
}

const target = readOption('target');
const arch = readOption('arch');
const sourceDirValue = readOption('source-dir');
const outputDirValue = readOption('output-dir');

if (!TARGETS.has(target)) {
  throw new Error(`Invalid --target: ${target || '(missing)'}`);
}
if (!arch) {
  throw new Error('--arch is required');
}
if (!sourceDirValue || !outputDirValue) {
  throw new Error('--source-dir and --output-dir are required');
}

const sourceDir = resolve(sourceDirValue);
const outputDir = resolve(outputDirValue);

function chooseSingle(predicate, description) {
  if (!existsSync(sourceDir)) {
    throw new Error(`Source directory does not exist: ${sourceDir}`);
  }
  const matches = readdirSync(sourceDir)
    .filter((name) => predicate(name))
    .sort();
  if (matches.length !== 1) {
    throw new Error(
      `Expected exactly one ${description} in ${sourceDir}, found ${matches.length}: ${matches.join(', ') || '(none)'}`,
    );
  }
  return matches[0];
}

function sourceAndAlias() {
  if (target === 'darwin') {
    if (!['aarch64', 'x86_64'].includes(arch)) {
      throw new Error(`Invalid macOS arch: ${arch}`);
    }
    const bundleArch = DARWIN_BUNDLE_ARCHES[arch];
    const sourceName = chooseSingle(
      (name) => name.endsWith(`_${bundleArch}.dmg`) && !name.startsWith('OpenLess_macos_'),
      `macOS ${arch} DMG`,
    );
    return {
      sourceName,
      aliasName: `OpenLess_macos_${arch}.dmg`,
    };
  }

  if (target === 'windows') {
    if (arch !== 'x86_64') {
      throw new Error(`Invalid Windows arch: ${arch}`);
    }
    const sourceName = chooseSingle(
      (name) => name.endsWith('_x64-setup.exe') && !name.startsWith('OpenLess_windows_'),
      'Windows x64 NSIS installer',
    );
    return {
      sourceName,
      aliasName: 'OpenLess_windows_x86_64-setup.exe',
    };
  }

  const abi = ANDROID_ABIS[arch];
  if (!abi) {
    throw new Error(`Invalid Android updater arch: ${arch}`);
  }
  const sourceName = chooseSingle(
    (name) => name.endsWith(`_${abi}.apk`) && !name.startsWith('OpenLess_android_'),
    `Android ${abi} APK`,
  );
  return {
    sourceName,
    aliasName: `OpenLess_android_${abi}.apk`,
  };
}

const { sourceName, aliasName } = sourceAndAlias();
mkdirSync(outputDir, { recursive: true });
const sourcePath = join(sourceDir, sourceName);
const aliasPath = join(outputDir, aliasName);
copyFileSync(sourcePath, aliasPath);

const signaturePath = `${sourcePath}.sig`;
if (existsSync(signaturePath)) {
  copyFileSync(signaturePath, `${aliasPath}.sig`);
}

console.log(`Staged ${basename(sourcePath)} as ${basename(aliasPath)}`);
