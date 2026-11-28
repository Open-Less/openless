import assert from 'node:assert/strict';
import { PreferencesWriteGate } from '../../../data/preferences-write-gate';
import { dictationReferenceKeyOptions, saveDictationReferencePreference } from './dictationReferenceKey';

assert.deepEqual(dictationReferenceKeyOptions.map(({ value }) => value), [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ']);
assert.equal(dictationReferenceKeyOptions.some(({ value }) => value === 'Space'), false);

const gate = new PreferencesWriteGate<{ dictationReferenceKey?: string }>();
gate.receiveIncoming({ revision: 1, preferences: {} });

const save = (key: string, reject: boolean) => saveDictationReferencePreference(async () => {
  const { id } = gate.beginWrite(gate.current(), { dictationReferenceKey: key });
  assert.equal(gate.current().dictationReferenceKey, key);
  if (reject) {
    gate.finishWrite(id);
    throw new Error('Shortcut conflict');
  }
  gate.finishWrite(id, { revision: 2, preferences: { dictationReferenceKey: key } });
});

assert.equal(await save('T', true), false);
assert.equal(gate.current().dictationReferenceKey ?? 'S', 'S');
assert.equal(await save('T', false), true);
assert.equal(await save('V', true), false);
assert.equal(gate.current().dictationReferenceKey, 'T');
