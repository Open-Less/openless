import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const imeRoot = join(appRoot, 'windows-ime', 'src');
const textService = readFileSync(join(imeRoot, 'text_service.cpp'), 'utf8');
const editSession = readFileSync(join(imeRoot, 'edit_session.cpp'), 'utf8');
const protocol = readFileSync(join(appRoot, 'src-tauri', 'src', 'windows_ime_protocol.rs'), 'utf8');

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
  ['kStatusAccepted', 'IME_STATUS_ACCEPTED'],
  ['kStatusPending', 'IME_STATUS_PENDING'],
  ['kStatusCommitted', 'IME_STATUS_COMMITTED'],
  ['kStatusUnknownToken', 'IME_STATUS_UNKNOWN_TOKEN'],
  ['kStatusBadRequest', 'IME_STATUS_BAD_REQUEST'],
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
  editSession,
  /InterlockedIncrement\(&g_object_count\)/,
  'IME edit sessions should keep the COM DLL loaded while TSF holds them',
);
assert.match(
  editSession,
  /InterlockedDecrement\(&g_object_count\)/,
  'IME edit sessions should release the COM DLL lifetime count when destroyed',
);
