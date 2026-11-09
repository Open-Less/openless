// Regression contract for the Windows WebView-creation dispatch boundary.
// This source contract cannot reproduce a native event-loop deadlock: Actions
// must also compile the real host. Source checks are not native installation acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (relative) => readFileSync(new URL(relative, import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const command = read('../src-tauri/src/commands/voice_edit.rs');


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
  assert.match(command, /#\[cfg\(target_os = "windows"\)\]\s*#\[tauri::command\]\s*pub async fn voice_edit_window_open\(/);
  assert.match(body, /show_voice_edit\(\);[\s\S]*Ok\(\(\)\)/);
  assert.doesNotMatch(body, /start_voice_edit_session\(/, 'opening must not start or commit a session');
});

test('non-Windows keeps its synchronous command and main-only presentation contract', () => {
  assert.match(command, /#\[cfg\(not\(target_os = "windows"\)\)\]\s*#\[tauri::command\]\s*pub fn voice_edit_window_open\(/);
  const start = command.indexOf('pub fn voice_edit_window_open(');
  const end = command.indexOf('\n#[tauri::command]', start);
  assert.ok(start >= 0 && end > start);
  const body = command.slice(start, end);
  assert.match(body, /window\.label\(\) != "main"/);
  assert.ok(body.indexOf('return Err(') < body.indexOf('show_voice_edit()'));
  assert.match(body, /show_voice_edit\(\);\s*Ok\(\(\)\)/);
  assert.doesNotMatch(body, /start_voice_edit_session\(|commit_voice_edit/);
});

for (const newline of ['\n', '\r\n']) {
  test(`command boundary is stable under ${JSON.stringify(newline)} checkout`, () => {
    const normalized = command.replace(/\n/g, newline).replace(/\r\n/g, '\n');
    assert.ok(openCommand(normalized).includes('show_voice_edit()'));
  });
}

test('missing boundaries fail and the following unrelated command is excluded', () => {
  assert.throws(() => openCommand('pub fn voice_edit_window_open() {}'));
  assert.throws(() => openCommand('pub async fn voice_edit_window_open() {}'));
  const fixture = 'pub async fn voice_edit_window_open() {}\n#[tauri::command]\npub fn unrelated() { commit_voice_edit(); }';
  assert.doesNotMatch(openCommand(fixture), /commit_voice_edit/);
});
