import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const imeRoot = join(appRoot, 'windows-ime', 'src');
const ipcClient = readFileSync(join(imeRoot, 'ipc_client.cpp'), 'utf8');
const ipcHeader = readFileSync(join(imeRoot, 'ipc_client.h'), 'utf8');
const textService = readFileSync(join(imeRoot, 'text_service.cpp'), 'utf8');
const editSession = readFileSync(join(imeRoot, 'edit_session.cpp'), 'utf8');

assert.doesNotMatch(
  ipcClient,
  /FlushFileBuffers\(/,
  'IME shutdown must not block the host UI thread waiting for the pipe client',
);
assert.match(
  ipcClient,
  /WaitForClientDisconnect/,
  'IME replies should wait for client disconnect through cancelable overlapped I/O',
);
assert.match(
  ipcHeader,
  /HRESULT Start\(/,
  'IME activation should report pipe server startup failures',
);
assert.match(
  ipcClient,
  /WaitForSingleObject\(startup_event_/,
  "IME activation must wait for the worker's first named-pipe creation result",
);
assert.match(
  ipcClient,
  /CreateNamedPipeW[\s\S]*ReportStartupResult/,
  'IME worker must report the first CreateNamedPipeW result to activation',
);
assert.match(
  ipcHeader,
  /void Run\(\) noexcept/,
  'IME worker exceptions must not terminate the host process',
);

assert.doesNotMatch(
  textService,
  /SendMessageTimeoutW\(/,
  'IME worker must not synchronously send a stack request to the owner thread',
);
assert.match(
  textService,
  /PostMessageW\(/,
  'IME worker should post an owned request to the owner thread',
);
assert.match(
  textService,
  /WaitForMultipleObjects\(/,
  'IME owner-thread and async edit waits should be cancelable during shutdown',
);
assert.match(
  textService,
  /PeekMessageW[\s\S]*PM_REMOVE/,
  'IME shutdown should release queued submit requests before destroying the message window',
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
