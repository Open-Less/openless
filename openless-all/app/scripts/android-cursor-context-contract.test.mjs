#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const read = (path) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8');

function braceBody(source, signature) {
  const signatureIndex = source.indexOf(signature);
  assert.notEqual(signatureIndex, -1, `missing: ${signature}`);
  const openBrace = source.indexOf('{', signatureIndex);
  let depth = 0;
  for (let index = openBrace; index < source.length; index += 1) {
    if (source[index] === '{') depth += 1;
    if (source[index] === '}') depth -= 1;
    if (depth === 0) return source.slice(openBrace + 1, index);
  }
  assert.fail(`missing closing brace: ${signature}`);
}

const ime = read('../android/kotlin/OpenLessImeService.kt');
const policy = read('../android/kotlin/ImePrivacyPolicy.kt');
const accessibility = read('../android/kotlin/OpenLessAccessibilityService.kt');
const bridge = read('../src-tauri/src/android/native_bridge.rs');
const dictationContext = read('../crates/openless-core/src/dictation_context.rs');
const types = read('../crates/openless-core/src/types.rs');

// The IME reads the caret context through its own InputConnection, and the
// switch is checked before the editor is touched at all.
const capture = braceBody(ime, 'private fun captureCursorContext()');
assert.match(capture, /currentInputConnection/, 'cursor context must come from the IME InputConnection');
assert.match(capture, /getTextBeforeCursor\(/);
assert.match(capture, /getTextAfterCursor\(/);
assert.doesNotMatch(capture, /Accessibility/, 'phase one must not depend on Accessibility');
assert.ok(
  capture.indexOf('cursorContextEnabled') !== -1 &&
    capture.indexOf('cursorContextEnabled') < capture.indexOf('currentInputConnection'),
  'the cursorContextEnabled switch must be checked before the editor is read',
);
assert.match(capture, /ImePrivacyPolicy\.captureCursorContext\(/, 'reads must go through the privacy gate');

// Privacy gate: password, TYPE_NULL, no-personalized-learning and sensitive apps.
const allows = policy.slice(policy.indexOf('fun allowsCursorContext'), policy.indexOf('fun captureCursorContext'));
assert.match(allows, /TYPE_NULL/);
assert.match(allows, /isPassword\(/);
assert.match(allows, /IME_FLAG_NO_PERSONALIZED_LEARNING/);
assert.match(allows, /isSensitivePackage\(/);
const gated = braceBody(policy, 'fun captureCursorContext(');
assert.ok(
  gated.indexOf('allowsCursorContext') < gated.indexOf('readBefore('),
  'the gate must run before any editor read',
);
assert.match(
  accessibility,
  /ImePrivacyPolicy\.isSensitivePackage\(/,
  'vocabulary observation must share the sensitive package list',
);

// Metadata-only logging: lengths and package, never the text itself.
for (const line of capture.split('\n').filter((line) => line.includes('Log.') || line.includes('_chars='))) {
  assert.doesNotMatch(line, /\.(before|after)\s*\}/, `cursor context text must not be logged: ${line.trim()}`);
}
const rustLog = bridge.slice(bridge.indexOf('[cursor-context] status=ok source=input_connection'));
assert.doesNotMatch(
  rustLog.slice(0, rustLog.indexOf(');')),
  /cursor_before|cursor_after|window\.text|window\.before\(\)\s*,|window\.after\(\)\s*,/,
  'the native log may only carry character counts',
);

// Only "start" carries the snapshot, and it is taken when recording starts.
const send = braceBody(ime, 'private fun sendImeCommand(');
assert.match(send, /if \(action == "start" && cursorContext != null\)/);
for (const key of ['frontApp', 'cursorBefore', 'cursorAfter']) assert.match(send, new RegExp(`put\\("${key}"`));
assert.match(ime, /sendImeCommand\("start", captureCursorContext\(\)\)/);
for (const action of ['stop', 'cancel', 'cloud']) {
  assert.doesNotMatch(
    ime,
    new RegExp(`sendImeCommand\\("${action}",`),
    `"${action}" must not resend cursor context`,
  );
}

// Kotlin hands over structured sides; Core owns the envelope format.
for (const [name, source] of [['OpenLessImeService.kt', ime], ['ImePrivacyPolicy.kt', policy], ['native_bridge.rs', bridge]]) {
  assert.doesNotMatch(source, /<cursor_context>/, `${name} must not build its own cursor_context envelope`);
}
const command = braceBody(bridge, 'struct ImeCommand');
for (const field of ['front_app', 'cursor_before', 'cursor_after']) {
  assert.match(command, new RegExp(`${field}: Option<String>`), `ImeCommand.${field}`);
}
const start = bridge.slice(bridge.indexOf('"start" => {'), bridge.indexOf('"cloud" => {'));
assert.match(start, /openless_core::host_document::window_from_split\(/);
assert.match(start, /openless_core::prompts::cursor_context_input\(/);
assert.match(start, /front_app: command\.front_app/);
assert.match(start, /\bcursor_context,/, 'the IME start must fill DictationStartOptions.cursor_context');

// Quick notes and cloud notes keep the context for polish...
const withTarget = braceBody(dictationContext, 'pub fn with_output_target(');
assert.doesNotMatch(withTarget, /cursor_context/, 'switching the output target must not drop cursor context');

// ...but nothing downstream of polish may carry it.
const completed = bridge.slice(bridge.indexOf('"kind":"completed"'));
assert.match(completed.slice(0, completed.indexOf('),')), /"text":result\.polished_text/);
assert.doesNotMatch(completed.slice(0, completed.indexOf('),')), /cursor/i);
const webhook = braceBody(ime, 'private fun submitCloudNoteText(');
assert.match(webhook, /put\("content", text\)/);
assert.doesNotMatch(webhook, /cursor|getTextBeforeCursor|getTextAfterCursor/i, 'the cloud note webhook must only send the note text');
const session = braceBody(types, 'pub struct DictationSession');
assert.doesNotMatch(session, /cursor/i, 'history entries must not gain a cursor context field');

console.log('android cursor context contract: ok');
