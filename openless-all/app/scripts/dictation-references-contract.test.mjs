import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const contract = JSON.parse(read('contract/backend-2.0.json')).dictationReferences;
assert.deepEqual(contract.preferences, { dictationReferencesEnabled: false });
assert.deepEqual(contract.hosts, ['macos', 'windows']);
assert.deepEqual(contract.capsule.referenceNotice, [null, 'tooLarge', 'captureFailed']);
assert.deepEqual(Object.keys(contract.commands), ['get_dictation_references_status']);
assert.deepEqual(contract.commands.get_dictation_references_status.result, { supported: 'boolean' });
const lib = read('src-tauri/src/lib.rs');
for (const name of Object.keys(contract.commands)) {
  assert.match(lib, new RegExp(`#\\[cfg\\(not\\(mobile\\)\\)\\]\\s+commands::${name}`));
  assert.ok(read('src-tauri/src/commands/dictation_references.rs').includes(`fn ${name}(`));
}
const prefs = read('crates/openless-core/src/shared_types.rs');
assert.match(prefs, /#\[serde\(default\)\]\s+pub dictation_references_enabled: bool/);
assert.match(prefs, /#\[serde\(default\)\]\s+pub reference_count: u32/);
assert.match(prefs, /#\[serde\(default\)\]\s+pub reference_notice: Option<String>/);
const api = read('crates/openless-core/src/api.rs');
assert.ok(api.indexOf('enrich_dictation_text(', api.indexOf('let mut engine_result')) < api.indexOf('.finish_text_insertion(session_id, engine_result.polished_text.clone())'));
const host = read('src-tauri/src/dictation_references.rs');
assert.doesNotMatch(host, /copy_intent|capture_busy|screenPermission|screencapture|CGPreflightScreenCaptureAccess|recording_companion/);
assert.ok(host.includes('own.revisions.contains(&revision)'));
assert.doesNotMatch(prefs, /pub dictation_reference_key:/);
assert.doesNotMatch(lib, /commands::open_dictation_references_permission_settings/);
assert.match(host, /supported: cfg!\(any\(target_os = "macos", target_os = "windows"\)\)/);
assert.match(host, /ClipboardFilter::new\(baseline\)/);
assert.match(host, /Duration::from_millis\(100\)/);
assert.match(host, /!bound \|\| !backend.get_preferences\(\).dictation_references_enabled/);
const windows = read('src-tauri/src/dictation_references/windows.rs');
for (const name of ['OpenClipboard', 'CloseClipboard', 'GlobalLock', 'GlobalUnlock', 'GlobalSize', 'dib_to_png', 'hdrop', 'unicode_text']) assert.ok(windows.includes(name));
assert.ok(read('src-tauri/src/insertion.rs').includes('dictation_references::self_write()'));
assert.ok(read('src-tauri/src/selection.rs').includes('dictation_references::self_write()'));
const adapters = read('src-tauri/src/core_adapters.rs');
assert.match(adapters, /prepare_streaming_input_source\(\s*context\.insertion\.streaming\s*&& !crate::dictation_references::active\(session_id\)/);
assert.match(adapters, /context\.insertion\.composition_input && context\.insertion\.enabled\s*&& !crate::dictation_references::active\(session_id\)/);
// Stop now freezes native content before slow recorder teardown (R19 F5),
// then persists that owned snapshot without rereading global CLIPBOARD.
assert.match(host, /freeze_clipboard\(&session, crate::insertion::clipboard_sequence, clipboard_content\)/);
const finish = host.slice(host.indexOf('impl StoppedCollection'), host.indexOf('impl Drop for RecordingLease'));
assert.match(finish, /snapshot\.content/);
assert.doesNotMatch(finish, /clipboard_sequence\(|clipboard_content\(|poll_revision\(/);
const guardDrop = host.slice(host.indexOf('impl Drop for SelfWrite'), host.indexOf('fn now_ms'));
assert.doesNotMatch(guardDrop, /revisions\.push|clipboard_sequence\(/);
assert.match(adapters, /references\.finish\(\)/);
assert.match(adapters, /selection_insertion_target_is_captured\(&insertion_target\)/);
console.log('dictation references backend contracts passed');
