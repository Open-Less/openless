import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(
  new URL('../harmony/entry/src/main/ets/VoiceConfiguration.ets', import.meta.url),
  'utf8',
);
function compile(source) {
  return ts.transpileModule(source.replace(/\r\n/g, '\n').replace(/^import .*;\n/gm, ''), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
}
const compiled = compile(source);
assert.equal(
  compile(source.replace(/\r?\n/g, '\r\n')),
  compiled,
  'Windows CRLF source uses the same isolated test host',
);
assert(!compiled.includes('require('), 'device-only imports are removed from the test host');
const records = new Map();
let snapshot = {
  version: 1,
  preferences: { workingLanguages: ['汉字'.repeat(700)] },
  channels: {},
};
let imported;
let exportsCount = 0;
let writes = 0;
let mutateOnRead;
const asset = {
  Tag: {
    ALIAS: 'alias',
    SECRET: 'secret',
    DATA_LABEL_NORMAL_1: 'label',
    ACCESSIBILITY: 'access',
    CONFLICT_RESOLUTION: 'conflict',
    RETURN_TYPE: 'return',
  },
  ReturnType: { ALL: 1, ATTRIBUTES: 2 },
  Accessibility: { DEVICE_UNLOCKED: 1 },
  ConflictResolution: { OVERWRITE: 1 },
  async add(attributes) {
    const secret = attributes.get('secret');
    assert(secret.length > 0 && secret.length <= 1024, 'respect native Asset Store size limits');
    records.set(new TextDecoder().decode(attributes.get('alias')), new Map(attributes));
    writes++;
  },
  async query(query) {
    const alias = query.get('alias');
    if (alias) {
      const name = new TextDecoder().decode(alias);
      if (mutateOnRead && !name.endsWith('.manifest')) {
        const mutate = mutateOnRead;
        mutateOnRead = undefined;
        await mutate();
      }
      if (!records.has(name)) throw Object.assign(new Error('not found'), { code: 24000002 });
      return [new Map(records.get(name))];
    }
    return [...records.values()].filter(
      (record) =>
        new TextDecoder().decode(record.get('label')) ===
        new TextDecoder().decode(query.get('label')),
    );
  },
  async remove(query) {
    records.delete(new TextDecoder().decode(query.get('alias')));
  },
};
function client() {
  const exports = {};
  vm.runInNewContext(compiled, {
    exports,
    console,
    asset,
    Uint8Array,
    Date,
    util: {
      TextEncoder: class {
        encodeInto(text) {
          return new TextEncoder().encode(text);
        }
      },
      TextDecoder: {
        create: () => ({
          decodeWithStream: (bytes) => new TextDecoder('utf-8', { fatal: true }).decode(bytes),
        }),
      },
    },
    nativeHost: {
      async invokeAsync(command, args) {
        if (command === '__export_voice_configuration') {
          exportsCount++;
          return JSON.stringify({ ok: true, value: snapshot });
        }
        assert.equal(command, '__import_voice_configuration');
        imported = JSON.parse(args);
        return JSON.stringify({ ok: true, value: true });
      },
    },
  });
  return exports;
}
const main = client(),
  ime = client();
await assert.rejects(ime.restoreVoiceConfiguration(), /请先打开/);
await main.publishVoiceConfiguration();
await ime.restoreVoiceConfiguration();
assert.deepEqual(imported, snapshot, 'UTF-8 survives chunk boundaries across independent runtimes');
const firstWrites = writes;
await main.publishVoiceConfiguration();
assert.equal(writes, firstWrites, 'unchanged settings avoid redundant vault writes');
snapshot = { ...snapshot, preferences: { workingLanguages: ['中文'], pipelineMode: 'multimodal' } };
mutateOnRead = () => main.publishVoiceConfiguration();
await ime.restoreVoiceConfiguration();
assert.deepEqual(
  imported,
  snapshot,
  'concurrent replacement retries instead of accepting mixed generations',
);
assert.equal(records.size, 2, 'obsolete chunks are cleaned up');
const manifest = [...records.keys()].find((key) => key.endsWith('.manifest'));
records
  .get(manifest)
  .set('secret', new TextEncoder().encode('{"generation":"1-1","chunks":999,"bytes":1}'));
await assert.rejects(ime.restoreVoiceConfiguration(), /同步失败/);
snapshot = { preferences: { text: 'x'.repeat(65536) } };
await assert.rejects(main.publishVoiceConfiguration(), /大小限制/);
assert(exportsCount >= 4);
console.log(
  'Harmony voice configuration: independent sandboxes, UTF-8 chunks, atomic publish, concurrent replacement and bounds passed',
);
