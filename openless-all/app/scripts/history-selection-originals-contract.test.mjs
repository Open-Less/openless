import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (name) => readFileSync(new URL(`../${name}`, import.meta.url), 'utf8');
const history = read('src/features/history/History.tsx');
const list = read('src/features/history/HistoryList.tsx');
const detail = read('src/features/history/HistoryDetail.tsx');
const originalSelection = read('src/features/history/useHistoryOriginalSelection.ts');
const originals = read('src/features/history/selectionOriginals.ts');
const presentation = read('src/features/history/presentation.ts');

assert.match(history, /quickNotesOnly \? emptyHistory : filtered/);
assert.match(history, /selectedId=\{item\?\.id\}\s+onSelect=\{selectRecord\}/);
assert.match(list, /type="checkbox"[\s\S]*?toggleOriginal\(session\.id\)/);
assert.match(list, /onClick=\{\(\) => onSelect\(session\.id\)\}/);
assert.match(detail, /showingRaw && hasSelectionOriginal\(item\)/);
assert.match(detail, /onClick=\{\(\) => void onCopy\(showingRaw\)\}/);
assert.match(history, /styleLabelFor\(entry, allPacks, modeLabel, t\)/);
assert.match(list, /styleLabel=\{styleLabel\(session\)\}/);
assert.match(list, /title=\{styleLabel\}/);
assert.match(detail, /title=\{styleLabel\(item\)\}/);
assert.match(presentation, /session\.source === 'selection_polish'/);
assert.match(presentation, /t\('history\.selectionPolishStyle', \{ style: packDisplayName\(pack, modeLabel\) \}\)/);
assert.match(presentation, /session\.source === 'selection_voice_edit'/);
assert.match(originalSelection, /selectionOriginalClipboardText\(entries, selectedIds\)/);
assert.match(originalSelection, /await copyTextToClipboard\(text\)/);
assert.match(
  originals,
  /\(entry\.source === 'selection_polish' \|\| entry\.source === 'selection_voice_edit'\)/,
);
assert.match(originals, /\.map\(\(entry\) => entry\.rawTranscript\)\s*\.join\('\\n\\n'\)/);
assert.doesNotMatch(originals, /\.trim\(|\b(?:finalText|summary)\b/);

for (const locale of ['zh-CN', 'zh-TW', 'en', 'de', 'es', 'fr', 'ja', 'ko']) {
  const source = read(`src/i18n/${locale}.ts`);
  for (const key of ['copyOriginal', 'selectOriginals', 'selectAllOriginals', 'copyOriginals',
    'selectionPolish', 'selectionPolishStyle', 'selectionVoiceEdit', 'voiceEdit']) {
    assert.match(source, new RegExp(`\\b${key}:\\s*['"][^'"]+`), `${locale} needs ${key}`);
  }
}

console.log('history original selection, copy and locale contracts passed');
