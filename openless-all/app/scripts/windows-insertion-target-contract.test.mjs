import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const adapters = await readFile(
  new URL('../src-tauri/src/core_adapters.rs', import.meta.url),
  'utf8',
);
const events = await readFile(new URL('../src-tauri/src/tauri_events.rs', import.meta.url), 'utf8');
const capsule = await readFile(new URL('../src/components/Capsule.tsx', import.meta.url), 'utf8');
const finalInsert = adapters.match(
  /async fn insert_final\([\s\S]*?(?=\n    async fn copy_fallback)/,
)?.[0];
assert.ok(finalInsert, 'the production final insertion path must exist');

// 原生接线合同：不改系统剪贴板或抢焦点，真实窗口效果仍需设备验收。
const unavailableTarget = finalInsert.match(
  /if let Err\(error\) = self\.restore_insertion_target\(\) \{([\s\S]*?)\n        \}/,
)?.[1];
assert.ok(
  unavailableTarget,
  'an unavailable original target must reach the configured copy-only fallback',
);
assert.match(
  unavailableTarget,
  /#\[cfg\(target_os = "windows"\)\]\s*if self\.context\.insertion\.allow_non_tsf_fallback\s*\{\s*return self\.copy_fallback\(text\)\.await;\s*\}/,
  'Windows may only copy when fallback is enabled and restoring the original target failed',
);
assert.match(
  unavailableTarget,
  /return Err\(error\);/,
  'disabled fallback must preserve the restore error',
);
assert.doesNotMatch(
  unavailableTarget,
  /insert_final|windows_unicode_fallback|\.insert\(/,
  'a failed target restore must never paste or type into the new focus',
);
assert.match(
  finalInsert,
  /Err\(error\) if error\.is_outcome_unknown\(\) => \{[\s\S]*?return Err\(BackendError::new\(\s*BackendErrorCode::OutcomeUnknown,/,
  'an uncertain TSF delivery must return before any fallback can duplicate it',
);
assert.match(
  events,
  /DictationInsertStatus::CopiedFallback => "已复制，请手动粘贴"/,
  'copied output must not be reported as an insertion failure',
);
assert.match(
  events,
  /BackendEventKind::InsertFallback\(fallback\) => \{[\s\S]*?show_core_insert_fallback\(text, &fallback\.reason\)/,
  'the fallback event must reach the native card',
);
assert.match(
  capsule,
  /'insert:fallback'[\s\S]*?setInsertFallback\(event\.payload \?\? null\)/,
  'the capsule must consume the fallback card payload',
);
assert.match(
  capsule,
  /return <InsertFallbackCard payload=\{insertFallback\} \/>/,
  'the copied text must remain visible in the fallback card',
);

const selection = await readFile(new URL('../src-tauri/src/selection.rs', import.meta.url), 'utf8');
const restoreTarget = selection.match(
  /pub\(crate\) fn reactivate_selection_insertion_target[\s\S]*?(?=\n    #\[cfg\(target_os = "macos"\)\])/,
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
