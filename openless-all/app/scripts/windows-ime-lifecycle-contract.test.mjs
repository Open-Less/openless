import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const imeRoot = join(appRoot, 'windows-ime', 'src');
const textService = readFileSync(join(imeRoot, 'text_service.cpp'), 'utf8');
const editSession = readFileSync(join(imeRoot, 'edit_session.cpp'), 'utf8');
const displayAttribute = readFileSync(join(imeRoot, 'display_attribute.cpp'), 'utf8');
const registry = readFileSync(join(imeRoot, 'registry.cpp'), 'utf8');
const protocol = readFileSync(join(appRoot, 'src-tauri', 'src', 'windows_ime_protocol.rs'), 'utf8');

// The TSF DLL is a standalone vcxproj that the Tauri build never touches, so the
// only thing that can catch a compile error before a maintainer opens Visual
// Studio is an explicit CI step.
const ciWorkflow = readFileSync(join(appRoot, '../../.github/workflows/ci.yml'), 'utf8');
assert.match(
  ciWorkflow,
  /msbuild OpenLessIme\.sln \/p:Configuration=Release \/p:Platform=x64/,
  'CI must compile the Windows TSF IME',
);
const ipc = readFileSync(join(appRoot, 'src-tauri', 'src', 'windows_ime_ipc.rs'), 'utf8');
const tauriLib = readFileSync(join(appRoot, 'src-tauri', 'src', 'lib.rs'), 'utf8');

// The IME runs inside every host process. It must not own threads or block the
// host UI thread: joining a worker from Deactivate deadlocked hosts whenever
// the calling thread held the loader lock.
const imeSources = readdirSync(imeRoot)
  .filter((name) => /\.(cpp|h)$/.test(name))
  .map((name) => [name, readFileSync(join(imeRoot, name), 'utf8')]);
for (const [name, source] of imeSources) {
  assert.doesNotMatch(
    source,
    /std::thread|_beginthreadex|CreateThread\(/,
    `${name}: IME must not create threads inside host processes`,
  );
  assert.doesNotMatch(
    source,
    /CreateNamedPipeW|ConnectNamedPipe/,
    `${name}: IME must not serve named pipes inside host processes`,
  );
  assert.doesNotMatch(
    source,
    /WaitForSingleObject|WaitForMultipleObjects|\.join\(\)|SendMessageTimeoutW\(|SendMessageW\(/,
    `${name}: IME must not block the host UI thread on waits or synchronous sends`,
  );
}
assert.ok(
  !existsSync(join(imeRoot, 'ipc_client.cpp')),
  'the in-host pipe server must stay removed',
);

assert.match(
  textService,
  /message == WM_COPYDATA/,
  'IME should receive submissions as WM_COPYDATA on its message window',
);
assert.match(
  textService,
  /PostMessageW\(message_window_, kRunSubmitMessage/,
  'IME should commit from a posted message at the top of the host message loop',
);
assert.doesNotMatch(
  textService,
  /ChangeWindowMessageFilter/,
  'IME must not let lower-integrity processes inject text into elevated hosts',
);
assert.match(
  textService,
  /STDMETHODIMP OpenLessTextService::Deactivate\(\) \{\s*(?:OpenLessTraceScope trace\(L"Deactivate"\);\s*)?CancelPendingSubmit\(\);\s*DestroyMessageWindow\(\);/,
  'IME deactivation should only cancel the pending submit and destroy its window',
);
assert.match(
  textService,
  /service->AddRef\(\);[\s\S]*service->Release\(\);/,
  'IME message handling should keep the service alive across re-entrant deactivation',
);

// The message protocol is defined twice (C++ and Rust); keep the copies equal.
const sharedConstants = [
  ['kCopyDataSubmit', 'IME_COPYDATA_SUBMIT'],
  ['kCopyDataQuery', 'IME_COPYDATA_QUERY'],
  ['kCopyDataUpdate', 'IME_COPYDATA_UPDATE'],
  ['kCopyDataFinish', 'IME_COPYDATA_FINISH'],
  ['kCopyDataCancel', 'IME_COPYDATA_CANCEL'],
  ['kCopyDataVersion', 'IME_COPYDATA_VERSION'],
  ['kStatusAccepted', 'IME_STATUS_ACCEPTED'],
  ['kStatusPending', 'IME_STATUS_PENDING'],
  ['kStatusCommitted', 'IME_STATUS_COMMITTED'],
  ['kStatusUnknownToken', 'IME_STATUS_UNKNOWN_TOKEN'],
  ['kStatusBadRequest', 'IME_STATUS_BAD_REQUEST'],
  ['kStatusNotDispatched', 'IME_STATUS_NOT_DISPATCHED'],
  ['kStatusCompositionConflict', 'IME_STATUS_COMPOSITION_CONFLICT'],
  ['kStatusCompositionRecoveryRequired', 'IME_STATUS_COMPOSITION_RECOVERY_REQUIRED'],
  ['kStatusProtocolVersion', 'IME_STATUS_PROTOCOL_VERSION'],
];
for (const [nativeName, rustName] of sharedConstants) {
  const nativeValue = textService.match(new RegExp(`${nativeName} = (0x[0-9A-Fa-f]+)`));
  const rustValue = protocol.match(new RegExp(`${rustName}: \\w+ = (0x[0-9A-Fa-f_]+)`));
  assert.ok(nativeValue, `${nativeName} should be defined in text_service.cpp`);
  assert.ok(rustValue, `${rustName} should be defined in windows_ime_protocol.rs`);
  assert.equal(
    Number(rustValue[1].replaceAll('_', '')),
    Number(nativeValue[1]),
    `${nativeName} and ${rustName} must match`,
  );
}
const windowClass = textService.match(/kMessageWindowClassName\[\] = L"([^"]+)"/);
assert.ok(windowClass, 'IME message window class should be defined');
assert.ok(
  protocol.includes(`OPENLESS_IME_MESSAGE_WINDOW_CLASS: &str = "${windowClass[1]}"`),
  'IME message window class must match between C++ and Rust',
);

assert.match(
  ipc,
  /option_env!\("OPENLESS_BUILD_ID"\)/,
  'TSF diagnostics should read the immutable build provenance injected by CI',
);
assert.ok(
  ipc.includes('\\"build_id\\":\\"{}\\"'),
  'P1/P2/P3 diagnostics should carry the bounded artifact build identity',
);
assert.match(
  ipc,
  /diagnostic_build_id\(\)/,
  'P1/P2/P3 diagnostics should sanitize the artifact build identity',
);
assert.ok(
  ipc.includes('\\"protocol_revision\\":\\"0x{:08X}\\"'),
  'P1/P2/P3 diagnostics should record the expected protocol revision',
);
assert.match(
  ipc,
  /IME_STATUS_PROTOCOL_VERSION/,
  'P1/P2/P3 diagnostics should use the current protocol revision constant',
);

assert.match(
  textService,
  /kCopyDataVersion[\s\S]*kStatusProtocolVersion/,
  'IME should answer the protocol probe so stale in-process DLLs are diagnosable',
);

assert.match(
  tauriLib,
  /let builder = builder\.plugin\(tauri_plugin_single_instance::init\(single_instance_callback\)\)/,
  'Every desktop platform must use the app-identifier-scoped single-instance broker',
);
assert.doesNotMatch(
  tauriLib,
  /windows_single_instance/,
  'Separate install paths must not bypass the shared single-instance guard',
);
assert.equal(
  JSON.parse(readFileSync(join(appRoot, 'src-tauri', 'tauri.conf.json'), 'utf8')).identifier,
  'com.openless.app',
  'All OpenLess copies must share the same single-instance identity',
);

assert.match(
  ipc,
  /fn ordered_ime_windows[\s\S]*window\.process_id == target\.process_id[\s\S]*window\.thread_id == target\.thread_id[\s\S]*allow_same_process_fallback/,
  'Rust discovery should order exact-thread candidates before recovery candidates',
);
assert.match(
  ipc,
  /select_ime_window_with_probe[\s\S]*verify_protocol_version\(/,
  'Rust discovery should probe each eligible message window before selecting it',
);
assert.match(
  ipc,
  /let window = find_ime_window_with_retry[\s\S]*send_copy_data\([\s\S]*request\.op\.copydata_tag/,
  'Rust should complete protocol discovery before sending a real submit payload',
);

assert.match(
  editSession,
  /InterlockedIncrement\(&g_object_count\)/,
  'IME edit sessions should keep the COM DLL loaded while TSF holds them',
);
assert.match(
  editSession,
  /InterlockedDecrement\(&g_object_count\)/,
  'IME edit sessions should release the COM DLL lifetime count when destroyed',
);

// Live transcript composition: TSF owns the composition, so the IME must start,
// update, and end it inside edit sessions, underline it, and learn when TSF ends it.
assert.match(
  editSession,
  /HRESULT OpenLessEditSession::GetSelectionRange[\s\S]*?InsertTextAtSelection\([\s\S]*?TF_IAS_QUERYONLY/,
  'IME should obtain a host-adjusted insertion range without modifying the document',
);
assert.match(
  editSession,
  /UpdateComposition[\s\S]*?SetText\([\s\S]*?Collapse\(edit_cookie, TF_ANCHOR_END\)[\s\S]*?context_->SetSelection/,
  'IME composition updates should keep the host selection at the end of the live text',
);
assert.match(
  editSession,
  /context_composition->StartComposition\(/,
  'IME should open the live transcript as a TSF composition',
);
assert.match(
  editSession,
  /composition_->composition->EndComposition\(/,
  'IME should end the composition when the final text replaces it',
);
assert.match(
  editSession,
  /ApplyOpenLessInputDisplayAttribute\(/,
  'IME composition text should carry the OpenLess input display attribute (underline)',
);
assert.match(
  displayAttribute,
  /ClearOpenLessInputDisplayAttribute[\s\S]*GetProperty\(GUID_PROP_ATTRIBUTE[\s\S]*property->Clear\(edit_cookie, range\)/,
  'IME must explicitly clear the display attribute instead of relying on EndComposition',
);
const cancellation = editSession.slice(
  editSession.indexOf('HRESULT OpenLessEditSession::CancelComposition'),
  editSession.indexOf('HRESULT OpenLessEditSession::FinishComposition'),
);
const finalization = editSession.slice(
  editSession.indexOf('HRESULT OpenLessEditSession::FinishComposition'),
);
assert.match(
  cancellation,
  /SetText[\s\S]*ClearOpenLessInputDisplayAttribute[\s\S]*EndComposition/,
  'Cancellation must clear the underline before releasing the restored selection',
);
assert.match(
  finalization,
  /ClearOpenLessInputDisplayAttribute\(edit_cookie, context_,\s*composition_->terminated_range\)/,
  'Finalization must clear the underline on an externally terminated range',
);
assert.match(
  finalization,
  /ClearOpenLessInputDisplayAttribute\(edit_cookie, context_, range\)[\s\S]*EndComposition/,
  'Finalization must clear the underline before ending a live composition',
);
const termination = textService.slice(
  textService.indexOf('STDMETHODIMP OpenLessTextService::OnCompositionTerminated'),
  textService.indexOf('STDMETHODIMP OpenLessTextService::EnumDisplayAttributeInfo'),
);
assert.match(
  termination,
  /ClearOpenLessInputDisplayAttribute[\s\S]*ReadOpenLessRangeText/,
  'External termination must clear the underline without waiting for dictation to finish',
);
assert.match(
  displayAttribute,
  /TF_LS_SOLID[\s\S]*TF_ATTR_INPUT/,
  'IME display attribute should paint a solid input underline',
);
assert.match(
  displayAttribute,
  /InterlockedIncrement\(&g_object_count\)/,
  'IME display attribute COM objects should keep the DLL alive while hosts hold them',
);
assert.match(
  textService,
  /IID_ITfDisplayAttributeProvider/,
  'IME should expose the display attribute provider for composition underline painting',
);
assert.match(
  registry,
  /GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER/,
  'IME registration should advertise the display attribute provider category',
);
assert.match(
  textService,
  /STDMETHODIMP OpenLessTextService::OnCompositionTerminated\(/,
  'IME should learn when TSF ends the composition',
);
assert.match(
  editSession,
  /terminated_text_captured[\s\S]*current_text[\s\S]*!= composition_->terminated_text/,
  'IME must preserve user edits made after TSF terminates a composition',
);
assert.match(
  textService,
  /ReadOpenLessRangeText\(edit_cookie, composition_->terminated_range[\s\S]*terminated_text/,
  'IME must snapshot the terminated composition text before finalization',
);
assert.match(
  textService,
  /OnCompositionTerminated[\s\S]*terminated_text_captured = true/,
  'IME must record whether the terminated composition snapshot was readable',
);
assert.match(
  editSession,
  /if \(!composition_->terminated_text_captured\)[\s\S]*ReadOpenLessRangeText\(edit_cookie, composition_->terminated_range[\s\S]*current_text != composition_->terminated_text[\s\S]*composition_->terminated_range->SetText\(/,
  'IME must verify terminated-range ownership before replacing it with final text',
);
assert.match(
  editSession,
  /else if \(composition_->terminated && composition_->terminated_range != nullptr\)[\s\S]*?else \{[\s\S]*?composition_->Reset\(\);[\s\S]*?return HRESULT_FROM_WIN32\(ERROR_OPERATION_ABORTED\);/,
  'IME cancellation must release latched native state when TSF terminates without a usable range',
);
assert.match(
  editSession,
  /if \(!composition_->terminated_text_captured\)[\s\S]*?return kStatusCompositionConflict;/,
  'IME must reject finalization when the terminated-range ownership snapshot is unavailable',
);
assert.match(
  editSession,
  /current_text != composition_->terminated_text[\s\S]*?return kStatusCompositionConflict;/,
  'IME must reject finalization when the user edits a terminated composition range',
);

assert.match(
  textService,
  /kCopyDataUpdate[\s\S]*AcceptSubmit\(copy_data, OpenLessEditOp::kUpdate\)/,
  'IME should route composition updates through the submit pipeline',
);
assert.match(
  textService,
  /submit_composition_owner_[\s\S]*CommitTextOnOwnerThread\([\s\S]*submit_composition_owner_/,
  'IME should carry a stable composition owner across submit requests',
);
assert.match(
  editSession,
  /owner_token[\s\S]*composition_owner_/,
  'IME edit sessions should associate composition state with its insertion owner',
);
assert.match(
  textService,
  /submit_composition_process_id_[\s\S]*CommitTextOnOwnerThread\([\s\S]*submit_composition_process_id_/,
  'IME should carry the sender process instance for restart recovery',
);
assert.match(
  textService,
  /kCommitHeaderBytes[\s\S]*composition_process_id[\s\S]*target_window/,
  'plain commits should carry sender process and captured focus for restart recovery',
);
assert.match(
  protocol,
  /encode_commit_submit_payload[\s\S]*sender_process[\s\S]*target_window/,
  'Rust should encode plain-commit recovery metadata',
);
assert.match(
  textService,
  /restart_recovery[\s\S]*kStatusCompositionRecoveryRequired/,
  'IME should require an explicit recovery cancel before restarting a composition',
);
assert.match(
  textService,
  /update_failed_before_composition[\s\S]*dispatched = false/,
  'IME should classify pre-StartComposition update failures as not dispatched',
);
assert.match(
  textService,
  /op == OpenLessEditOp::kCancel[\s\S]*op == OpenLessEditOp::kFinish[\s\S]*accepted async update[\s\S]*dispatched = false/,
  'IME cleanup should clear a latch when async composition never started',
);
assert.doesNotMatch(
  textService,
  /ITfComposition[^\n]*\bWait/,
  'IME composition handling must not wait on the host',
);
