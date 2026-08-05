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

const pasteBody = kotlinFunctionBody('private fun performPasteToFocusedFieldInternal(pasteText: String? = null)');
assert.match(
  pasteBody,
  /finally\s*\{\s*target\.recycle\(\)\s*\}/s,
  'paste completion must recycle only the per-call target',
);
assert.doesNotMatch(
  pasteBody,
  /finally\s*\{[\s\S]*?invalidateEditableCache\s*\(/,
  'paste completion must retain the validated focus cache for a consecutive paste',
);

const targetBody = kotlinFunctionBody('private fun findEditableTarget()');
const rootBody = kotlinFunctionBody('private fun findEditableInRoot(root: AccessibilityNodeInfo)');
assert.match(
  targetBody,
  /lastEditableFocus\?\.let\s*\{\s*cached\s*->[\s\S]*?OpenLessAccessibilityTarget\.isPasteTarget\(cached\)/s,
  'findEditableTarget must try lenient cached paste target before window scans',
);
assert.match(
  targetBody,
  /for\s*\(\s*window\s+in\s+windows\s*\)/,
  'findEditableTarget must scan all accessibility windows',
);
assert.match(
  rootBody,
  /editableFocusedNode\(root, AccessibilityNodeInfo\.FOCUS_INPUT\)\?\.let\s*\{\s*fresh\s*->\s*cacheEditableTarget\(fresh\)\s*return fresh/s,
  'a fresh focus target must refresh the service cache',
);
assert.match(
  rootBody,
  /lastEditableFocus\?\.let\s*\{\s*cached\s*->[\s\S]*?OpenLessAccessibilityTarget\.isValidCachedEditable\(cached, root\)[\s\S]*?return AccessibilityNodeInfo\.obtain\(cached\)/s,
  'cached focus reuse must retain package, window, focus, and refresh validation',
);
assert.match(
  rootBody,
  /editableFocusedNode\(root, AccessibilityNodeInfo\.FOCUS_ACCESSIBILITY\)/,
  'findEditableInRoot must try accessibility focus after input focus',
);
assert.match(
  rootBody,
  /findEditableInTree\(root, 0\)/,
  'findEditableInRoot must fall back to editable tree search',
);

assert.match(source, /pasteAppearsApplied/, 'paste must verify editor text changed');
assert.match(source, /paste=unverified/, 'paste must log unverified ACTION_PASTE results');
assert.match(
  readFileSync(
    fileURLToPath(new URL('../android/kotlin/OpenLessAccessibilityCommandReceiver.kt', import.meta.url)),
    'utf8',
  ),
  /EXTRA_PASTE_TEXT/,
  'paste IPC must carry text to the accessibility process',
);
assert.match(source, /EXTRA_PASTE_TEXT/, 'paste sender must include IPC text extra');

console.log('Android accessibility paste cache contract checks passed');
