import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(new URL('../harmony/entry/src/main/ets/inputmethod/RimeClient.ets', import.meta.url), 'utf8');
const stripImports = text => text.replace(/\r\n/g, '\n').replace(/^import .*;\n/gm, '');
assert.equal(stripImports(source), stripImports(source.replace(/\r?\n/g, '\r\n')));
const compiled = ts.transpileModule(stripImports(source), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

function host({ selected = '', rejectSnapshot = false } = {}) {
  const files = new Map(), requests = [];
  const initializationPaths = [], dictionaryCommands = [];
  let handle, calls = 0, fail = false;
  const expected = new TextEncoder().encode('dictionary-中文-contents');
  // Resource APIs may return a view into a larger buffer.
  const backing = new Uint8Array(expected.length + 8).fill(255);
  backing.set(expected, 4);
  const view = backing.subarray(4, 4 + expected.length);
  const fs = {
    OpenMode: { CREATE: 1, WRITE_ONLY: 2, TRUNC: 4 },
    accessSync: path => files.has(path), mkdirSync: path => files.set(path, []),
    async open(path) { handle = path; files.set(path, []); return { fd: 1 }; },
    async write(_fd, buffer) {
      calls++;
      if (fail) { fail = false; return 0; }
      const bytes = new Uint8Array(buffer), count = Math.min(7, bytes.length);
      files.get(handle).push(...bytes.subarray(0, count));
      return count;
    },
    async close() {},
    async rename(from, to) { files.set(to, files.get(from)); files.delete(from); },
  };
  const exports = {};
  vm.runInNewContext(compiled, {
    exports, console, fs,
    util: { TextDecoder: { create: () => ({ decodeToString: bytes => new TextDecoder().decode(bytes) }) } },
    nativeHost: {
      initBackend: () => '{"ok":true}',
      async invokeAsync(command) {
        dictionaryCommands.push(command);
        return JSON.stringify({ ok: true, value: { data_path: command === 'ime_dictionary_select' ? selected : '/profile/rime-shared-1.17.0-ice-da1fbe60-v1' } });
      },
      async rimeAsync(operation, path) {
        requests.push(operation); initializationPaths.push(path);
        return JSON.stringify({ ok: !(rejectSnapshot && path === selected), error: 'invalid public dictionary fixture' });
      },
    },
  });
  const resources = {
    async getRawFileContent(name) {
      return name.endsWith('manifest.json') ? new TextEncoder().encode('["test.dict.yaml"]') : view;
    },
  };
  return { exports, files, requests, resources, expected, initializationPaths, dictionaryCommands, writes: () => calls, failNext: () => { fail = true; } };
}

const h = host();
await Promise.all([h.exports.initializeRime('/profile', h.resources), h.exports.initializeRime('/profile', h.resources)]);
assert.deepEqual(h.requests, ['initialize'], 'prewarm and editor attachment share one deployment');
const path = '/profile/rime-shared-1.17.0-ice-da1fbe60-v1/test.dict.yaml';
assert.deepEqual(h.files.get(path), Array.from(h.expected), 'partial writes and resource view bounds preserve every byte');
const writes = h.writes();
await h.exports.initializeRime('/profile', h.resources);
assert.equal(h.writes(), writes, 'ready dictionaries must not be recopied');

const retry = host();
retry.failNext();
await assert.rejects(retry.exports.initializeRime('/profile', retry.resources));
assert.equal(retry.files.has(path), false, 'incomplete data must never become the cached resource');
assert.deepEqual(retry.requests, []);
await retry.exports.initializeRime('/profile', retry.resources);
assert.deepEqual(retry.files.get(path), Array.from(retry.expected));
assert.deepEqual(retry.requests, ['initialize']);
const base = '/profile/rime-shared-1.17.0-ice-da1fbe60-v1';
const selected = base + '/public-updates-v1/' + 'a'.repeat(40);
const updated = host({ selected });
await updated.exports.initializeRime('/profile', updated.resources, 'rime-user', '/cache');
assert.deepEqual(updated.initializationPaths, [selected]);
const rollback = host({ selected, rejectSnapshot: true });
await rollback.exports.initializeRime('/profile', rollback.resources, 'rime-user', '/cache');
assert.deepEqual(rollback.initializationPaths, [selected, base]);
assert.deepEqual(rollback.dictionaryCommands, ['ime_dictionary_select', 'ime_dictionary_reject']);
const invalidPath = host({ selected: '/profile/rime-user' });
await invalidPath.exports.initializeRime('/profile', invalidPath.resources, 'rime-user', '/cache');
assert.deepEqual(invalidPath.initializationPaths, [base], 'public data selection must not redirect to the personal profile');
console.log('Harmony Rime startup: shared initialization, asynchronous partial writes, atomic resources and failure retry passed');
