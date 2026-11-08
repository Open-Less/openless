// Regression contract for the Windows WebView-creation dispatch boundary.
// This source contract cannot reproduce a native event-loop deadlock: Actions
// must also compile the real host, and the affected device must verify N7/N8.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (relative) => readFileSync(new URL(relative, import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const command = read('../src-tauri/src/commands/voice_edit.rs');
const diagnostics = read('../src-tauri/src/issue1338_debug.rs');
const host = read('../src-tauri/src/lib.rs');

function openCommand(source) {
  const start = source.indexOf('pub async fn voice_edit_window_open(');
  assert.notEqual(start, -1, 'window creation must not run inside a synchronous IPC command');
  const end = source.indexOf('\n#[tauri::command]', start);
  assert.notEqual(end, -1, 'the next command must bound extraction');
  return source.slice(start, end);
}

test('voice edit creation leaves the synchronous IPC handler and retains main-only authorization', () => {
  const body = openCommand(command);
  assert.match(body, /window\.label\(\) != "main"/);
  assert.match(body, /return Err\("Voice Edit can only be opened from the main window"\.to_string\(\)\)/);
  assert.ok(body.indexOf('return Err(') < body.indexOf('show_voice_edit()'));
  assert.match(body, /observe_future\([\s\S]*Kind::VoiceEditOpen[\s\S]*async move/);
  assert.match(body, /show_voice_edit\(\);[\s\S]*Ok\(\(\)\)/);
  assert.doesNotMatch(body, /start_voice_edit_session\(/, 'opening must not start or commit a session');
});

test('diagnostic source preserves the specific open and native-stage boundaries', () => {
  assert.match(diagnostics, /"voice_edit_window_open"\s*=>\s*Some\(Self::VoiceEditOpen\)/);
  for (const kind of ['VoiceEditLookup', 'VoiceEditCreate', 'VoiceEditFocusable', 'VoiceEditShow']) {
    assert.ok(host.includes(`Kind::${kind}`), `${kind} must observe the real native stage`);
  }
  assert.match(diagnostics, /request_intent": "unknown"/);
  assert.match(diagnostics, /response_application": "unknown"/);
});

for (const newline of ['\n', '\r\n']) {
  test(`command boundary is stable under ${JSON.stringify(newline)} checkout`, () => {
    const normalized = command.replace(/\n/g, newline).replace(/\r\n/g, '\n');
    assert.ok(openCommand(normalized).includes('show_voice_edit()'));
  });
}
