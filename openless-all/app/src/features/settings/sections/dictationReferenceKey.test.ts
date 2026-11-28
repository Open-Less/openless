import { PreferencesWriteGate } from '../../../data/preferences-write-gate';
import { dictationReferenceKeyOptions, saveDictationReferencePreference } from './dictationReferenceKey';

function assertEqual(actual: unknown, expected: unknown) {
  if (actual !== expected) throw new Error(`Expected ${expected}, got ${actual}`);
}

assertEqual(dictationReferenceKeyOptions.map(({ value }) => value).join(','), [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'].join(','));
assertEqual(dictationReferenceKeyOptions.some(({ value }) => value === 'Space'), false);

const gate = new PreferencesWriteGate<{ dictationReferenceKey?: string }>();
gate.receiveIncoming({ revision: 1, preferences: {} });

const save = (key: string, reject: boolean) => saveDictationReferencePreference(async () => {
  const { id } = gate.beginWrite(gate.current(), { dictationReferenceKey: key });
  if (reject) {
    gate.finishWrite(id);
    throw new Error('Shortcut conflict');
  }
  gate.finishWrite(id, { revision: 2, preferences: { dictationReferenceKey: key } });
});

assertEqual(await save('T', true), false);
assertEqual(gate.current().dictationReferenceKey ?? 'S', 'S');
assertEqual(await save('T', false), true);
assertEqual(await save('V', true), false);
assertEqual(gate.current().dictationReferenceKey, 'T');
