#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const servicePath = fileURLToPath(
  new URL('../android/kotlin/OpenLessImeService.kt', import.meta.url),
);
const copyScriptPath = fileURLToPath(new URL('./copy-android-scaffolding.mjs', import.meta.url));
const service = readFileSync(servicePath, 'utf8');
const copyScript = readFileSync(copyScriptPath, 'utf8');

function functionBody(source, signature) {
  const signatureIndex = source.indexOf(signature);
  assert.notEqual(signatureIndex, -1, `missing function: ${signature}`);
  const openBrace = source.indexOf('{', signatureIndex);
  assert.notEqual(openBrace, -1, `missing opening brace: ${signature}`);
  let depth = 0;
  for (let index = openBrace; index < source.length; index += 1) {
    if (source[index] === '{') depth += 1;
    if (source[index] === '}') depth -= 1;
    if (depth === 0) return source.slice(openBrace + 1, index);
  }
  assert.fail(`missing closing brace: ${signature}`);
}

const submit = functionBody(service, 'private fun submitCloudNoteText');
const report = functionBody(service, 'private fun reportCloudNoteFailure');
const successStart = submit.indexOf('if (success)');
const successElse = submit.indexOf('} else {', successStart);
assert.ok(successStart !== -1 && successElse > successStart, 'missing webhook success branch');
const successBranch = submit.slice(successStart, successElse);

assert.match(submit, /val success = code in 200\.\.299/);
assert.match(submit, /CloudNoteFailure\.MissingDestination/);
assert.match(submit, /CloudNoteFailure\.Http/);
assert.match(submit, /CloudNoteFailure\.Network/);
// The draft workflow must receive a completion even when a newer request owns the status line.
assert.match(
  submit,
  /val message = reportCloudNoteFailure\(requestId, text, CloudNoteFailure\.MissingDestination\)\s+onFinished\?\.invoke\(false, message\)/,
);
assert.match(
  submit,
  /val message = reportCloudNoteFailure\(requestId, text, CloudNoteFailure\.Network\)\s+onFinished\?\.invoke\(false, message\)/,
);
assert.match(
  submit,
  /reportCloudNoteFailure\(requestId, text, CloudNoteFailure\.Http, code\)\s+}\s+onFinished\?\.invoke\(success, message\)/,
);
assert.match(successBranch, /if \(requestId == imeRequestId\)/);
assert.doesNotMatch(
  submit,
  /return@post/,
  'stale submissions must still finish their draft callback',
);
assert.match(report, /return message/);
assert.match(
  report,
  /val persisted = retained && OpenLessClipboardHistory\.recordCopy\(this, text\)/,
);
assert.match(report, /cloudNoteFailureMessage\([\s\S]*?retained,\s+persisted,/);
assert.match(report, /cloudNoteShouldRetain\(text\)/);
assert.doesNotMatch(
  successBranch,
  /recordCopy|reportCloudNoteFailure|cloudNoteShouldRetain/,
  'a successful webhook submit must not keep a local copy',
);
assert.match(copyScript, /'CloudNoteDelivery\.kt'/);
assert.match(copyScript, /'CloudNoteDeliveryTest\.kt'/);
assert.match(copyScript, /'OpenLessClipboardHistoryInstrumentedTest\.kt'/);

console.log('android cloud note delivery contract passed');
