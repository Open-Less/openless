#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const ime = readFileSync(
  new URL('../android/kotlin/OpenLessImeService.kt', import.meta.url),
  'utf8',
);

function body(signature) {
  const start = ime.indexOf(signature);
  assert.notEqual(start, -1, `missing: ${signature}`);
  const open = ime.indexOf('{', start);
  let depth = 0;
  for (let index = open; index < ime.length; index++) {
    if (ime[index] === '{') depth++;
    if (ime[index] === '}') depth--;
    if (depth === 0) return ime.slice(open + 1, index);
  }
  assert.fail(`missing closing brace: ${signature}`);
}

test('only a current host dictation is saved before the draft changes the target', () => {
  const open = body('private fun openDictationEditDraft(');
  assert.match(
    open,
    /val restoreText = lastDictationText\?\.takeIf \{ sessionEpoch == lastDictationEpoch \}/,
  );
  assert.match(open, /val restoreUndone = restoreText != null && dictationTextUndone/);
  assert.ok(open.indexOf('val restoreText') < open.indexOf('openDraft('));
  assert.match(open, /draftRestoreDictationText = restoreText/);
  assert.match(open, /draftRestoreDictationUndone = restoreUndone/);
});

test('discard cancels draft dictation before restoring the host result at the current epoch', () => {
  const discard = body('private fun discardDraft()');
  assert.match(discard, /if \(recording \|\| processing\) cancelDictation\(\)/);
  assert.ok(discard.indexOf('cancelDictation()') < discard.indexOf('closeDraft()'));
  assert.ok(discard.indexOf('closeDraft()') < discard.indexOf('lastDictationText = restoreText'));
  assert.match(discard, /lastDictationEpoch = sessionEpoch/);
  assert.match(discard, /dictationTextUndone = restoreUndone/);
  assert.doesNotMatch(discard, /\bsessionEpoch\s*(?:=(?!=)|\+\+|--|[+-]=)/);
  assert.doesNotMatch(ime, /draftRestoreDictationEpoch/);
});

test('input restart cancels the session and closes the draft before initializing input', () => {
  const start = body('override fun onStartInput(');
  assert.match(start, /^\s*cancelImeSession\(\)\s+closeDraft\(\)\s+super\.onStartInput\(/);
  for (const signature of ['override fun onFinishInputView(', 'override fun onFinishInput()']) {
    assert.match(body(signature), /closeDraft\(\)/);
  }
});

test('closing a draft invalidates callbacks and clears its connection and submit lock', () => {
  const close = body('private fun closeDraft()');
  assert.match(close, /if \(draftEdit == null\) return/);
  for (const statement of [
    'draftToken++',
    'draftEdit = null',
    'draftConnection = null',
    'draftSubmitting = false',
  ]) {
    assert.ok(close.includes(statement), `missing cleanup: ${statement}`);
  }
  const submit = body('private fun submitCloudNoteDraft(');
  const guard =
    'if (token != draftToken || submissionRequestId != imeRequestId) return@submitCloudNoteText';
  assert.ok(submit.includes(guard));
  assert.ok(submit.indexOf(guard) < submit.indexOf('draftSubmitting = false'));
});

test('restored undo and editing retain their host epoch, selection and exact-text checks', () => {
  const undo = body('private fun toggleUndoRedoDictation()');
  assert.match(undo, /if \(sessionEpoch != lastDictationEpoch\) return/);
  assert.match(undo, /requireCollapsedDictationCursor\(/);
  assert.ok(
    undo.indexOf('DictationSpan.matchesBeforeCursor(') <
      undo.indexOf('connection.deleteSurroundingText('),
  );
  const open = body('private fun openDictationEditDraft(');
  assert.match(
    open,
    /val resultIntact = sessionEpoch == lastDictationEpoch && !dictationTextUndone/,
  );
  assert.match(open, /lastDictationText\?\.takeIf \{ resultIntact \}/);
});
