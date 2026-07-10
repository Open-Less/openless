#!/usr/bin/env node
/**
 * Sign release APK files with minisign (same key as desktop updater).
 * Expects TAURI_SIGNING_PRIVATE_KEY (+ optional password) in env.
 *
 * Usage: node scripts/sign-android-apks.mjs <apk> [<apk> ...]
 */
import { existsSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import process from 'node:process';

function requireSigningKey() {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
    throw new Error('TAURI_SIGNING_PRIVATE_KEY is required to sign Android APKs');
  }
}

function signApk(apkPath) {
  if (!existsSync(apkPath)) {
    throw new Error(`APK not found: ${apkPath}`);
  }
  const sigPath = `${apkPath}.sig`;
  if (existsSync(sigPath)) {
    console.log(`Already signed: ${sigPath}`);
    return sigPath;
  }

  // Prefer `tauri signer sign` (bundled minisign wrapper used by desktop release).
  const tauriBin = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  const result = spawnSync(
    tauriBin,
    ['tauri', 'signer', 'sign', apkPath],
    {
      stdio: 'inherit',
      env: process.env,
      shell: process.platform === 'win32',
    },
  );
  if (result.status !== 0) {
    throw new Error(`tauri signer sign failed for ${apkPath} (exit ${result.status ?? 'unknown'})`);
  }
  if (!existsSync(sigPath)) {
    throw new Error(`Signature file missing after sign: ${sigPath}`);
  }
  console.log(`Signed ${apkPath} -> ${sigPath}`);
  return sigPath;
}

function main() {
  requireSigningKey();
  const apks = process.argv.slice(2);
  if (apks.length === 0) {
    console.error('Usage: node scripts/sign-android-apks.mjs <apk> [<apk> ...]');
    process.exit(1);
  }
  for (const apk of apks) {
    signApk(apk);
  }
}

main();
