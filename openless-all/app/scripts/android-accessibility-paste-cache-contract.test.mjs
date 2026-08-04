#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const servicePath = fileURLToPath(
  new URL('../android/kotlin/OpenLessAccessibilityService.kt', import.meta.url),
);
const source = readFileSync(servicePath, 'utf8');

function kotlinFunctionBody(functionSignature) {
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

const pasteBody = kotlinFunctionBody('private fun performPasteToFocusedFieldInternal()');
assert.match(
  pasteBody,
  /rootInActiveWindow[\s\S]*findFocus\(AccessibilityNodeInfo\.FOCUS_INPUT\)[\s\S]*findFocus\(AccessibilityNodeInfo\.FOCUS_ACCESSIBILITY\)/s,
  'paste must resolve focus from the active window only',
);
assert.match(source, /pasteAppearsApplied/, 'paste must verify editor text changed');
assert.match(source, /paste=unverified/, 'paste must log unverified ACTION_PASTE results');
assert.match(pasteBody, /pasteWithRetryOrSetText\(focused\)/, 'paste must retry ACTION_PASTE then SET_TEXT');
assert.doesNotMatch(source, /lastEditableFocus/, 'paste path must not keep an editable focus cache');
assert.doesNotMatch(source, /findEditableInTree/, 'paste path must not walk the accessibility tree');
assert.doesNotMatch(source, /rememberFocusedEditable/, 'paste path must not warm a focus cache from events');
assert.doesNotMatch(pasteBody, /for\s*\(\s*window\s+in\s+windows\s*\)/, 'paste must not scan all windows');

console.log('Android accessibility paste contract checks passed');
