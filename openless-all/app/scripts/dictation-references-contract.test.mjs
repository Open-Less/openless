import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const contract = JSON.parse(read('contract/backend-2.0.json')).dictationReferences;
assert.deepEqual(contract.preferences, { dictationReferencesEnabled: false, dictationReferenceKey: 'S' });
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
assert.ok(host.includes('copy_intent'));
assert.ok(host.includes('own.revisions.contains(&revision)'));
assert.ok(host.includes('capture_busy'));
assert.ok(read('src-tauri/src/insertion.rs').includes('dictation_references::self_write()'));
assert.ok(read('src-tauri/src/selection.rs').includes('dictation_references::self_write()'));
const adapters = read('src-tauri/src/core_adapters.rs');
assert.match(adapters, /prepare_streaming_input_source\(\s*context\.insertion\.streaming\s*&& !crate::dictation_references::active\(session_id\)/);
assert.match(adapters, /context\.insertion\.composition_input && context\.insertion\.enabled\s*&& !crate::dictation_references::active\(session_id\)/);
assert.ok(host.includes('poll(self.id)'));
console.log('dictation references backend contracts passed');
