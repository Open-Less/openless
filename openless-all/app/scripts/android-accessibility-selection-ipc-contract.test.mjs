#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const servicePath = fileURLToPath(
  new URL('../android/kotlin/OpenLessAccessibilityService.kt', import.meta.url),
);
const receiverPath = fileURLToPath(
  new URL('../android/kotlin/OpenLessAccessibilityCommandReceiver.kt', import.meta.url),
);
const serviceSource = readFileSync(servicePath, 'utf8');
const receiverSource = readFileSync(receiverPath, 'utf8');

function kotlinFunctionBody(source, functionSignature) {
  const signatureIndex = source.indexOf(functionSignature);
  assert.notEqual(signatureIndex, -1, `missing Kotlin function: ${functionSignature}`);
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

const captureBody = kotlinFunctionBody(serviceSource, 'fun captureSelectedText(): String');
assert.match(
  captureBody,
  /instance\?\.let\s*\{\s*return it\.captureSelectedTextFromFocusedNode\(\)\s*\}/s,
  'the accessibility process must keep the direct instance read',
);
assert.match(
  captureBody,
  /return captureSelectedTextFromAccessibilityProcess\(\)/,
  'a main-process call must fall back to the explicit accessibility-process IPC path',
);

assert.match(
  receiverSource,
  /const val ACTION_CAPTURE_SELECTED_TEXT = "com\.openless\.app\.accessibility\.CAPTURE_SELECTED_TEXT"/,
  'receiver must expose a dedicated selection action',
);
assert.match(
  receiverSource,
  /const val EXTRA_SELECTED_TEXT = "selected_text"/,
  'receiver must expose a stable selected-text Bundle key',
);

const receiverActionBody = kotlinFunctionBody(
  receiverSource,
  'ACTION_CAPTURE_SELECTED_TEXT ->',
);
assert.match(
  receiverActionBody,
  /OpenLessAccessibilityService\.captureSelectedTextFromCommand\(\)/,
  'selection receiver action must read from the service-process instance',
);
assert.match(
  receiverActionBody,
  /putString\(EXTRA_SELECTED_TEXT, selectedText\.orEmpty\(\)\)/,
  'selection receiver action must return text using the declared Bundle key',
);
assert.doesNotMatch(
  receiverActionBody,
  /performPasteFromCommand|pasteToFocusedField/,
  'selection receiver action must never invoke paste',
);

const ipcBody = kotlinFunctionBody(
  serviceSource,
  'private fun captureSelectedTextFromAccessibilityProcess(): String',
);
assert.match(
  ipcBody,
  /action = OpenLessAccessibilityCommandReceiver\.ACTION_CAPTURE_SELECTED_TEXT/,
  'selection IPC sender must target the dedicated receiver action',
);
assert.match(
  ipcBody,
  /getString\(OpenLessAccessibilityCommandReceiver\.EXTRA_SELECTED_TEXT\)/,
  'selection IPC sender must read the receiver Bundle key',
);
assert.match(
  ipcBody,
  /latch\.await\(SELECTION_COMMAND_TIMEOUT_MS, TimeUnit\.MILLISECONDS\)/,
  'selection IPC must use a bounded wait',
);
assert.match(
  ipcBody,
  /else\s*\{\s*Log\.w\(TAG, "accessibility selection command timed out"\)\s*""\s*\}/s,
  'selection IPC timeout must return an empty selection',
);
assert.doesNotMatch(
  ipcBody,
  /ACTION_PASTE|performPasteFromCommand|pasteToFocusedField/,
  'selection IPC must not fall through to paste',
);

console.log('Android accessibility selection IPC contract checks passed');
