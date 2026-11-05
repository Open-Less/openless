import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('../assets/remote-input/app.js', import.meta.url), 'utf8');
const names = [
  'readReconnectToken',
  'rememberReconnectToken',
  'clearReconnectToken',
  'reconnectPhone',
  'connect',
  'handleMessage',
];
const production = names
  .map((name) => {
    const start = source.indexOf(`  function ${name}(`);
    const end = source.indexOf('\n  }', start);
    assert.ok(start >= 0 && end > start, `${name} must exist in the phone page`);
    return source.slice(start, end + '\n  }'.length);
  })
  .join('\n');

function phone({ storageAvailable = true } = {}) {
  const storage = new Map([['ol_remote_pin', '123456']]);
  const sent = [];
  const calls = { closed: 0, recordingScreen: 0, recovery: 0 };
  const state = {
    PIN_KEY: 'ol_remote_pin',
    RECONNECT_TOKEN_KEY: 'ol_remote_reconnect_token',
    reconnectToken: '',
    authed: false,
    recording: false,
    awaitingResult: false,
    busy: false,
    mode: 'toggle',
    pinInput: { value: '123456' },
    location: { host: '192.168.1.2:8443' },
    L: { errPinLocked: 'locked', errPinWrong: 'wrong' },
    localStorage: {
      getItem: (key) => storage.get(key) ?? null,
      setItem(key, value) {
        if (!storageAvailable) throw new Error('storage disabled');
        storage.set(key, value);
      },
      removeItem: (key) => storage.delete(key),
    },
    WebSocket: class {
      constructor(url) {
        this.url = url;
      }
    },
    wsSendJSON: (message) => sent.push(JSON.parse(JSON.stringify(message))),
    closeWS() {
      calls.closed++;
    },
    enterRecScreen() {
      calls.recordingScreen++;
    },
    requestRecovery() {
      calls.recovery++;
    },
    showScreen() {},
    showPinError() {},
    resetConnectBtn() {},
    armConnectTimeout() {},
    clearConnectTimeout() {},
  };
  vm.createContext(state);
  vm.runInContext(production, state);
  return { state, storage, sent, calls };
}

test('initial pairing sends the PIN and stores only the returned reconnect credential', () => {
  const { state, storage, sent, calls } = phone();
  state.connect('123456');
  state.ws.onopen();
  assert.deepEqual(sent, [{ type: 'hello', prefer: 'toggle', pin: '123456' }]);
  state.handleMessage({ type: 'auth', ok: true, reconnectToken: 'a'.repeat(64) });
  assert.equal(state.authed, true);
  assert.equal(storage.get('ol_remote_reconnect_token'), 'a'.repeat(64));
  assert.equal(storage.has('ol_remote_pin'), false);
  assert.equal(state.pinInput.value, '');
  assert.equal(calls.recovery, 1);
});

test('reconnection after a PIN change uses the token and never replays the old PIN', () => {
  const { state, sent } = phone();
  state.rememberReconnectToken('b'.repeat(64));
  state.reconnectPhone();
  state.ws.onopen();
  assert.deepEqual(sent, [{ type: 'hello', prefer: 'toggle', reconnectToken: 'b'.repeat(64) }]);
  assert.equal(Object.hasOwn(sent[0], 'pin'), false);
  state.handleMessage({ type: 'auth', ok: true });
  assert.equal(state.reconnectToken, 'b'.repeat(64));
});

test('page reload reads a token while a legacy PIN alone cannot reconnect', () => {
  const { state, storage, calls } = phone();
  assert.equal(state.readReconnectToken(), '');
  state.reconnectPhone();
  assert.equal(calls.closed, 0);
  storage.set('ol_remote_reconnect_token', 'c'.repeat(64));
  assert.equal(state.readReconnectToken(), 'c'.repeat(64));
  state.reconnectPhone();
  assert.equal(calls.closed, 1);
});

test('temporary lockout retains the token; explicit invalidation clears it', () => {
  const { state, storage } = phone();
  state.rememberReconnectToken('d'.repeat(64));
  state.handleMessage({ type: 'auth', ok: false, reason: 'locked' });
  assert.equal(storage.get('ol_remote_reconnect_token'), 'd'.repeat(64));
  state.handleMessage({ type: 'auth', ok: false, reason: 'bad-token' });
  assert.equal(state.reconnectToken, '');
  assert.equal(storage.has('ol_remote_reconnect_token'), false);
});

test('blocked browser storage still permits reconnection for the current page', () => {
  const { state, sent } = phone({ storageAvailable: false });
  state.rememberReconnectToken('e'.repeat(64));
  state.reconnectPhone();
  state.ws.onopen();
  assert.equal(sent[0].reconnectToken, 'e'.repeat(64));
});

test('saving a reconnect credential does not restart an active recording or pending result', () => {
  const { state, calls } = phone();
  state.recording = true;
  state.awaitingResult = true;
  state.rememberReconnectToken('f'.repeat(64));
  assert.equal(state.recording, true);
  assert.equal(state.awaitingResult, true);
  assert.equal(calls.closed, 0);
});
