import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

function sliceBetween(source, start, end, name) {
  const startIndex = source.indexOf(start);
  assert.notEqual(startIndex, -1, `${name}: missing start marker ${start}`);
  const endIndex = source.indexOf(end, startIndex + start.length);
  assert.notEqual(endIndex, -1, `${name}: missing end marker ${end}`);
  return source.slice(startIndex, endIndex);
}

function assertOrdered(source, fragments, name) {
  let cursor = -1;
  for (const fragment of fragments) {
    const next = source.indexOf(fragment, cursor + 1);
    assert.notEqual(next, -1, `${name}: missing or out of order fragment ${fragment}`);
    cursor = next;
  }
}

function assertMatch(source, pattern, name) {
  assert.match(source, pattern, name);
}

const profileRs = await readFile(
  new URL("../src-tauri/src/windows_ime_profile.rs", import.meta.url),
  "utf8",
);
const sessionRs = await readFile(
  new URL("../src-tauri/src/windows_ime_session.rs", import.meta.url),
  "utf8",
);
const coordinatorRs = await readFile(
  new URL("../src-tauri/src/coordinator.rs", import.meta.url),
  "utf8",
);
const dictationRs = await readFile(
  new URL("../src-tauri/src/coordinator/dictation.rs", import.meta.url),
  "utf8",
);

const activateOpenLess = sliceBetween(
  profileRs,
  "pub fn activate_openless_profile() -> WindowsImeProfileResult<()> {",
  "pub fn restore_profile(snapshot: &ImeProfileSnapshot) -> WindowsImeProfileResult<()> {",
  "activate_openless_profile",
);
assertOrdered(
  activateOpenLess,
  [
    "profiles.EnableLanguageProfile(&clsid, OPENLESS_TSF_LANG_ID, &profile_guid, true)?;",
    "profiles.ChangeCurrentLanguage(OPENLESS_TSF_LANG_ID)?;",
    "profiles.ActivateLanguageProfile(&clsid, OPENLESS_TSF_LANG_ID, &profile_guid)",
    "manager.ActivateProfile(",
    "TF_PROFILETYPE_INPUTPROCESSOR",
  ],
  "OpenLess activation should update legacy TSF before modern TSF",
);

const restoreProfile = sliceBetween(
  profileRs,
  "pub fn restore_profile(snapshot: &ImeProfileSnapshot) -> WindowsImeProfileResult<()> {",
  "pub fn is_openless_profile_active() -> WindowsImeProfileResult<bool> {",
  "restore_profile",
);
const restoreTextService = sliceBetween(
  restoreProfile,
  "ImeProfileKind::TextService => {",
  "ImeProfileKind::KeyboardLayout => {",
  "restore_profile text service branch",
);
assertOrdered(
  restoreTextService,
  [
    "let lang_id = snapshot.lang_id();",
    "profiles.ChangeCurrentLanguage(lang_id)?;",
    "profiles.ActivateLanguageProfile(&clsid, lang_id, &profile_guid)",
    "let modern_result = with_profile_manager",
    "manager.ActivateProfile(",
    "TF_PROFILETYPE_INPUTPROCESSOR",
    "if let Err(err) = modern_result",
    "[windows-ime] legacy restore OK but modern ActivateProfile failed",
  ],
  "saved text-service restore should mirror activation and log modern TSF drift",
);
const restoreKeyboard = restoreProfile.slice(
  restoreProfile.indexOf("ImeProfileKind::KeyboardLayout => {"),
);
assertOrdered(
  restoreKeyboard,
  [
    "let lang_id = snapshot.lang_id();",
    "profiles.ChangeCurrentLanguage(lang_id)",
    "let modern_result = with_profile_manager",
    "manager.ActivateProfile(",
    "TF_PROFILETYPE_KEYBOARDLAYOUT",
    "if let Err(err) = modern_result",
    "[windows-ime] legacy restore OK but modern ActivateProfile (keyboard) failed",
  ],
  "saved keyboard-layout restore should restore legacy language before modern TSF bookkeeping",
);

const prepareSession = sliceBetween(
  sessionRs,
  "pub fn prepare_session(&self) -> PreparedWindowsImeSession {",
  "pub async fn submit_prepared(",
  "prepare_session",
);
assertOrdered(
  prepareSession,
  [
    "self.profile_manager.capture_active_profile()",
    "self.profile_manager.activate_openless_profile()",
    "saved_profile: Some(saved_profile)",
    "PreparedWindowsImeSession::activation_failed(saved_profile)",
  ],
  "Windows IME session should snapshot the user profile before activating OpenLess",
);

const restoreSession = sliceBetween(
  sessionRs,
  "pub fn restore_session(&self, prepared: PreparedWindowsImeSession) {",
  "#[cfg(test)]",
  "restore_session",
);
assertOrdered(
  restoreSession,
  [
    "self.profile_manager.is_openless_profile_active()",
    "restore_decision(",
    "ProfileRestoreDecision::RestoreSavedProfile",
    "self.profile_manager.restore_profile(saved_profile)",
    "[windows-ime] restore saved profile failed",
  ],
  "Windows IME restore session should make a restore decision and log API-level restore failures",
);

const insertWithWindowsImeFirst = sliceBetween(
  coordinatorRs,
  "async fn insert_with_windows_ime_first(",
  "fn should_try_non_tsf_insertion_fallback(",
  "insert_with_windows_ime_first",
);
assertOrdered(
  insertWithWindowsImeFirst,
  [
    "take_matching_prepared_windows_ime_session(&mut slot, session_id)",
    "inner.windows_ime.submit_prepared(&prepared, request).await",
    "inner.windows_ime.restore_session(prepared);",
    "insert_via_non_tsf_fallback(inner, polished, restore_clipboard, paste_shortcut)",
  ],
  "TSF submit should restore the saved IME before falling back to non-TSF insertion",
);

const beginSession = sliceBetween(
  dictationRs,
  "pub(super) async fn begin_session(inner: &Arc<Inner>) -> Result<(), String> {",
  "pub(super) async fn start_recorder_and_enter_listening(",
  "dictation begin_session",
);
assertOrdered(
  beginSession,
  [
    "let prepared = inner.windows_ime.prepare_session();",
    "store_prepared_windows_ime_session(&mut slots, current_session_id, prepared);",
    "if let Err(message) = ensure_asr_credentials()",
    "restore_prepared_windows_ime_session(inner, current_session_id);",
    "if let Err(message) = ensure_microphone_permission(inner)",
    "restore_prepared_windows_ime_session(inner, current_session_id);",
  ],
  "begin_session should prepare IME early and restore it on pre-recorder errors",
);

const finishInsert = sliceBetween(
  dictationRs,
  "let status = if already_streamed {",
  "let inserted_chars = polished.chars().count() as u32;",
  "dictation finish insert",
);
assertOrdered(
  finishInsert,
  [
    "InsertStatus::Inserted",
    "insert_with_windows_ime_first(",
    "inner.inserter.copy_fallback(&polished)",
    "restore_prepared_windows_ime_session(inner, current_session_id);",
  ],
  "dictation success, TSF fallback, copy fallback, and streamed insert paths should converge on IME restore",
);

assertMatch(
  dictationRs,
  /if inner\.state\.lock\(\)\.cancelled \{[\s\S]*?\[coord\] cancel detected after ASR[\s\S]*?restore_prepared_windows_ime_session\(inner, current_session_id\);[\s\S]*?return Ok\(\(\)\);/,
  "cancel after ASR should restore the prepared Windows IME session",
);
assertMatch(
  dictationRs,
  /pub\(super\) fn cancel_session\(inner: &Arc<Inner>\) \{[\s\S]*?cancel_asr_for_session\(inner, decision\.session_id\);[\s\S]*?restore_prepared_windows_ime_session\(inner, decision\.session_id\);/,
  "explicit cancel_session should restore the prepared Windows IME session",
);

const currentSessionRestoreCalls =
  dictationRs.match(/restore_prepared_windows_ime_session\(inner, current_session_id\);/g) ?? [];
assert.ok(
  currentSessionRestoreCalls.length >= 20,
  `dictation should keep broad Windows IME cleanup coverage; found ${currentSessionRestoreCalls.length} current-session restore calls`,
);
