#!/usr/bin/env node
/**
 * Ensure Android updater minisign pubkey matches tauri.conf.json plugins.updater.pubkey.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const confPath = join(appRoot, 'src-tauri/tauri.conf.json');
const logicPath = join(appRoot, 'src-tauri/src/android/updater_logic.rs');

const conf = JSON.parse(readFileSync(confPath, 'utf8'));
const confPubkey = conf?.plugins?.updater?.pubkey;
if (!confPubkey) {
  console.error('check-android-updater-pubkey: missing plugins.updater.pubkey in tauri.conf.json');
  process.exit(1);
}

const logicSource = readFileSync(logicPath, 'utf8');
const match = logicSource.match(
  /pub const UPDATER_PUBKEY_B64: &str\s*=\s*"([^"]+)"/,
);
if (!match) {
  console.error('check-android-updater-pubkey: UPDATER_PUBKEY_B64 not found in updater_logic.rs');
  process.exit(1);
}

const rustPubkey = match[1];
if (rustPubkey !== confPubkey) {
  console.error('check-android-updater-pubkey: pubkey mismatch');
  console.error('  tauri.conf.json:', confPubkey);
  console.error('  updater_logic.rs:', rustPubkey);
  process.exit(1);
}

console.log('check-android-updater-pubkey: OK');
