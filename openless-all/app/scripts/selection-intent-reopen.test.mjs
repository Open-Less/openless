// Exercise a reused native intent picker; only React/IPC/event boundaries are replaced.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const app = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(import.meta.url);
const ts = require('typescript');
const source = readFileSync(app + '/src/pages/SelectionVoiceIntentPicker.tsx', 'utf8');
const start = source.indexOf('export function SelectionVoiceIntentPicker()');
const end = source.indexOf('\n  return (', start);
assert(start >= 0 && end >= 0);
const body =
  source
    .slice(start, end)
    .replace('export function', 'function')
    .replace("import('@tauri-apps/api/event')", 'Promise.resolve({listen})') +
  'return {choose,cancel};\n}';
const compiled = ts.transpileModule(body, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
}).outputText;
const factory = new Function(
  'useState',
  'useEffect',
  'useTranslation',
  'getSelectionVoiceIntentPrompt',
  'confirmSelectionVoiceIntentPrompt',
  'cancelSelectionVoiceIntentPrompt',
  'listen',
  'isTauri',
  compiled + ';return SelectionVoiceIntentPicker();',
);
const settle = () => new Promise((resolve) => setImmediate(resolve));
let failures = 0;
for (const action of ['choose', 'cancel']) {
  const state = [];
  const callbacks = new Map();
  const useState = (initial) => {
    const index = state.length;
    state.push(initial);
    return [
      initial,
      (value) => {
        state[index] = value;
      },
    ];
  };
  const component = factory(
    useState,
    (effect) => effect(),
    () => ({ t: (key) => key }),
    async () => ({ instruction: 'Question', sourceText: 'Selected text' }),
    async () => {},
    async () => {},
    async (name, callback) => {
      callbacks.set(name, callback);
      return () => {};
    },
    true,
  );
  await settle();
  if (action === 'choose') await component.choose('question');
  else await component.cancel();
  assert.equal(state[2], true);
  callbacks.get('selection-voice-intent:shown')();
  await settle();
  try {
    assert.equal(state[2], false, action + ' then hide/show must restore button usability');
    console.log(action + ': PASS');
  } catch (error) {
    console.log(action + ': FAIL - ' + error.message);
    failures++;
  }
}
console.log(`${failures} lifecycle regression(s) reproduced`);
process.exitCode = failures ? 1 : 0;
