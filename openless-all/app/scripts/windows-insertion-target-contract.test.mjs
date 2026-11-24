import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const adapters = await readFile(
  new URL('../src-tauri/src/core_adapters.rs', import.meta.url),
  'utf8',
);
const events = await readFile(new URL('../src-tauri/src/tauri_events.rs', import.meta.url), 'utf8');
const capsuleFeedback = await readFile(
  new URL('../src-tauri/src/capsule_feedback.rs', import.meta.url),
  'utf8',
);
const capsule = await readFile(
  new URL('../src/features/capsule/Capsule.tsx', import.meta.url),
  'utf8',
);
const dictationApi = await readFile(new URL('../src/api/dictation.ts', import.meta.url), 'utf8');
const finalInsert = adapters.match(
  /async fn insert_final\([\s\S]*?(?=\n    async fn copy_fallback)/,
)?.[0];
assert.ok(finalInsert, 'the production final insertion path must exist');
const unicodeFallback = adapters.match(
  /fn windows_unicode_fallback\([\s\S]*?(?=\nfn map_insert_status)/,
)?.[0];
assert.match(
  unicodeFallback ?? '',
  /matches!\(status, crate::types::InsertStatus::Inserted \| crate::types::InsertStatus::PasteSent\)/,
  'partially dispatched Unicode input must remain uncertain instead of becoming copy-only fallback',
);

// Published stable captures its destination before credentials, then restores it.
// Native preparation stays deferred so overlapping recordings cannot own input state.
const ordinaryInserter = adapters.match(
  /impl CoreTextInserter for TauriTextInserter \{[\s\S]*?(?=\n#\[derive\(Clone\)\]\nstruct TauriTextInsertionSession)/,
)?.[0];
assert.ok(ordinaryInserter);
assert.match(ordinaryInserter,
  /fn defer_preparation_until_stop\(&self\) -> bool\s*\{\s*true\s*\}/);
const captureTarget = ordinaryInserter.match(/fn capture_target\([\s\S]*?(?=\n    fn begin)/)?.[0];
assert.match(captureTarget ?? '', /insertion_target: Some\(insertion_target.clone\(\)\)/);
assert.match(captureTarget ?? '', /capture_ime_submit_target/);
assert.doesNotMatch(captureTarget ?? '', /capture_dictation_insertion_target/);
const ordinaryTargetCheck = adapters.match(
  /fn restore_insertion_target\([\s\S]*?(?=\n    async fn write_chunk)/,
)?.[0];
assert.match(ordinaryTargetCheck ?? '', /reactivate_dictation_insertion_target/);
assert.match(finalInsert, /if let Err\(error\) = self.restore_insertion_target\(\)/);
assert.match(finalInsert, /let mut target = crate::windows_ime_target::capture_ime_submit_target/);
assert.doesNotMatch(finalInsert, /KeyboardDelivery::capture|insert_confirmed/,
  'stable ordinary paste must not depend on an AX receipt');
assert.match(finalInsert, /InsertStatus::PasteSent[\s\S]*?InsertStatus::Inserted/,
  'macOS stable treats a successful paste dispatch as inserted');
assert.match(adapters, /fn supports_streaming[\s\S]*?self.streaming_ready/,
  'native polish chunks must remain enabled when stable preferences permit them');
assert.match(
  finalInsert,
  /InsertStatus::Failed\s*&& self\.context\.insertion\.allow_non_tsf_fallback[\s\S]*?windows_unicode_fallback/,
  'known TSF failure may use the configured Unicode fallback',
);
assert.doesNotMatch(
  unicodeFallback ?? '',
  /selection_insertion_target_is_current/,
  'ordinary Unicode fallback must type at current focus, not require the recording-time range',
);
assert.match(
  finalInsert,
  /Err\(error\) if error\.is_outcome_unknown\(\) => \{[\s\S]*?return Err\(BackendError::new\(\s*BackendErrorCode::OutcomeUnknown,/,
  'an uncertain TSF delivery must return before any fallback can duplicate it',
);
assert.match(
  events,
  /BackendEventKind::DictationCompleted\(result\) => \{[\s\S]*?dictation_completion_payload\(\s*&result,[\s\S]*?present_core_capsule\(payload\)/,
  'the completion event must present the payload produced by the capsule feedback policy',
);
const completionPayload = capsuleFeedback.match(
  /fn dictation_completion_payload\([\s\S]*?(?=\n#\[derive)/,
)?.[0];
assert.ok(completionPayload, 'the capsule feedback policy must build a completion payload');
assert.match(
  completionPayload,
  /DictationInsertStatus::CopiedFallback => "已复制，请手动粘贴"/,
  'copied output must not be reported as an insertion failure',
);
assert.match(
  events,
  /BackendEventKind::InsertFallback\(fallback\) => \{[\s\S]*?show_core_insert_fallback\(text, &fallback\.reason\)/,
  'the fallback event must reach the native card',
);
assert.match(
  dictationApi,
  /fallback:[\s\S]*?events\.listen\('insert:fallback', onPayload, 'window'\)/,
  'fallback events must remain bound to the current capsule window',
);
assert.match(
  capsule,
  /events\.fallback\([\s\S]*?setInsertFallback\(payload \?\? null\)/,
  'the capsule must consume the fallback card payload',
);
assert.match(
  capsule,
  /return <InsertFallbackCard key=\{insertFallback.presentationId\} payload=\{insertFallback\} \/>/,
  'the copied text must remain visible in the fallback card',
);

const selection = await readFile(new URL('../src-tauri/src/selection.rs', import.meta.url), 'utf8');
const selectionApply = adapters.match(
  /impl SelectionPlatformBridge for NativeSelectionPlatformBridge[\s\S]*?(?=\n#\[cfg\(not\(mobile\)\)\]\nstruct TauriSelectionRuntime)/,
)?.[0];
assert.match(
  selectionApply ?? '',
  /validate_selection_insertion_target\(target, source_text\)[\s\S]*?if validation\.error_code\(\)\.is_some\(\)[\s\S]*?return crate::selection::selection_result_fallback[\s\S]*?crate::selection::insert_selection_result\(/,
  'selection replacement must validate the original target and full source before writing',
);
const selectionValidation = selection.match(
  /pub\(crate\) fn validate_selection_insertion_target\([\s\S]*?\n\}/,
)?.[0];
assert.match(
  selectionValidation ?? '',
  /selection_insertion_target_is_current\(target\)/,
  'selection replacement must still verify its original control and range',
);
assert.match(
  selectionValidation ?? '',
  /selection_text_matches\(expected_selection,/,
  'selection validation must receive the full original text',
);
const positionCheck = selection.match(
  /pub\(crate\) fn selection_insertion_target_is_current\([\s\S]*?\n\}/,
)?.[0];
assert.match(
  positionCheck ?? '',
  /target_position::unchanged\([\s\S]*?focused_input_position\(\)/,
  'selection target validation must retain the exact live-range comparison',
);
const restoreTarget = selection.match(
  /fn reactivate_insertion_target[\s\S]*?(?=\n    #\[cfg\(target_os = "macos"\)\])/,
)?.[0];
assert.ok(restoreTarget, 'the production Windows target restore path must exist');
assert.match(
  restoreTarget,
  /if IsIconic\(foreground\)\.as_bool\(\)\s*\{\s*let _ = ShowWindow\(foreground, SW_RESTORE\);\s*\}[\s\S]*?BringWindowToTop\(foreground\)[\s\S]*?SetForegroundWindow\(foreground\)/,
  'a minimized original window must be restored before activation',
);
assert.match(
  restoreTarget,
  /return capture_windows_selection_target\(\)\.as_ref\(\) == Some\(&captured\);/,
  'restoring a window must still verify the original window and control fingerprint',
);

console.log('Windows insertion target contract passed');
