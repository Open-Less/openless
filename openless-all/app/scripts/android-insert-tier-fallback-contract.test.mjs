#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const insertPath = fileURLToPath(
  new URL('../src-tauri/src/android/insert.rs', import.meta.url),
);
const tiersPath = fileURLToPath(
  new URL('../src-tauri/src/android/insert_tiers.rs', import.meta.url),
);
const shizukuPath = fileURLToPath(
  new URL('../src-tauri/src/android/shizuku.rs', import.meta.url),
);
const bridgePath = fileURLToPath(
  new URL('../android/kotlin/OpenLessShizukuBridge.kt', import.meta.url),
);
const clientPath = fileURLToPath(
  new URL('../android/kotlin/OpenLessShizukuUserServiceClient.kt', import.meta.url),
);
function rustFunctionBody(source, functionSignature) {
  const signatureIndex = source.indexOf(functionSignature);
  assert.notEqual(signatureIndex, -1, `missing Rust function: ${functionSignature}`);
  const openBrace = source.indexOf('{', signatureIndex);
  assert.notEqual(openBrace, -1, `missing opening brace: ${functionSignature}`);

  let depth = 0;
  for (let index = openBrace; index < source.length; index += 1) {
    if (source[index] === '{') depth += 1;
    if (source[index] === '}') depth -= 1;
    if (depth === 0) return source.slice(openBrace + 1, index);
  }
  assert.fail(`missing closing brace: ${functionSignature}`);
}

const insertSource = readFileSync(insertPath, 'utf8');
const tiersSource = readFileSync(tiersPath, 'utf8');
const shizukuSource = readFileSync(shizukuPath, 'utf8');
const bridgeSource = readFileSync(bridgePath, 'utf8');
const clientSource = readFileSync(clientPath, 'utf8');
const tieredFallbackBody = rustFunctionBody(insertSource, 'fn insert_with_tiered_fallback');

assert.match(
  insertSource,
  /insert_with_tiered_fallback/,
  'android insert must use the tiered fallback entry point',
);
assert.match(
  tieredFallbackBody,
  /paste_via_accessibility_with_result\(text\)[\s\S]*paste_via_shizuku_with_result\(\)/s,
  'tier1 accessibility must be attempted before tier2 shizuku',
);
assert.match(
  tieredFallbackBody,
  /paste_via_shizuku_with_result\(\)[\s\S]*resolve_tiered_insert_status/s,
  'tier resolution must happen after shizuku',
);
assert.match(
  tieredFallbackBody,
  /paste_via_accessibility_with_result\(text\)[\s\S]*paste_via_shizuku_with_result\(\)[\s\S]*clipboard_fallback\(/s,
  'clipboard fallback must run only after accessibility and shizuku attempts',
);
assert.match(
  tiersSource,
  /resolve_tiered_insert_status/,
  'tier resolution helper must exist for clipboard fallback gating',
);
assert.match(
  shizukuSource,
  /paste_via_shizuku_with_result/,
  'shizuku module must expose paste injection result',
);
assert.match(
  tieredFallbackBody,
  /tier2 skipped: tier1 succeeded/,
  'tier2 must be skipped when tier1 already succeeded',
);
assert.match(
  clientSource,
  /processNameSuffix\(/,
  'recovery service bind must set processNameSuffix',
);
assert.match(
  clientSource,
  /withPasteService/,
  'shizuku client must expose daemon paste service bind',
);
assert.match(
  clientSource,
  /withPasteService[\s\S]*daemon\s*=\s*true[\s\S]*PASTE_SERVICE_PROCESS_SUFFIX/s,
  'paste service bind must use daemon mode with required process suffix',
);

assert.match(
  bridgeSource,
  /fun injectPasteKey\(context: Context\): Boolean/,
  'shizuku bridge must expose injectPasteKey',
);
assert.match(
  bridgeSource,
  /injectPasteKeyViaShizukuShell/,
  'paste injection must try Shizuku shell before UserService bind',
);
assert.match(
  bridgeSource,
  /getDeclaredMethod\(\s*"newProcess"/,
  'shell paste must invoke private Shizuku.newProcess via reflection',
);

console.log('Android insert tier fallback contract checks passed');
