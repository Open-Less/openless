// Exercise a reused native intent picker; only React/IPC/event boundaries are replaced.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const app = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(import.meta.url);
const ts = require('typescript');
const selectionApiSource = ts.createSourceFile(
  'selection.ts',
  readFileSync(app + '/src/api/selection.ts', 'utf8'),
  ts.ScriptTarget.ES2022,
  true,
);
const selectionApi = ts.transpileModule(
  selectionApiSource.statements
    .filter((node) => !ts.isImportDeclaration(node) && !ts.isExportDeclaration(node))
    .map((node) => node.getFullText(selectionApiSource))
    .join('\n'),
  { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } },
).outputText;
const makeListen = new Function(
  'listenTauriEvent',
  'exports',
  `${selectionApi};return listenSelectionVoiceIntentShown;`,
);
const source = readFileSync(
  app + '/src/features/selection-intent/SelectionVoiceIntentPicker.tsx',
  'utf8',
);
const start = source.indexOf('export function SelectionVoiceIntentPicker()');
const end = source.indexOf('\n  return (', start);
assert(start >= 0 && end >= 0);
const body =
  source.slice(start, end).replace('export function', 'function') +
  'return {choose,cancel: () => cancel.current?.()};\n}';
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
  'listenSelectionVoiceIntentShown',
  'useRef',
  'useModalActive',
  'window',
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
    makeListen(async (name, callback) => {
      callbacks.set(name, callback);
      return () => {};
    }, {}),
    (current) => ({ current }),
    () => false,
    { addEventListener() {}, removeEventListener() {} },
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
