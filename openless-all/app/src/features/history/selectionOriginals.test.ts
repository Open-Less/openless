import { hasSelectionOriginal, selectionOriginalClipboardText } from './selectionOriginals';

function expect(actual: unknown, expected: unknown) {
  if (actual !== expected) {
    throw new Error('Selection originals did not preserve record order or verbatim text');
  }
}

const longOriginal = ` \n${'a'.repeat(25_000)}\nMIDDLE-SENTINEL\n${'b'.repeat(25_000)}\t \n`;
const records = [
  { id: 'failed', source: 'selection_polish', rawTranscript: longOriginal, errorCode: 'failed' },
  { id: 'voice', source: 'voice', rawTranscript: 'Transcript' },
  { id: 'legacy', rawTranscript: 'Original of unknown source' },
  { id: 'missing', source: 'selection_polish', rawTranscript: '' },
  { id: 'quickNote', source: 'quick_note', rawTranscript: 'Quick note' },
  {
    id: 'complete',
    source: 'selection_polish',
    rawTranscript: ' \tSecond original \n',
    finalText: 'Edited result',
  },
] as const;

expect(hasSelectionOriginal(records[0]), true);
expect(hasSelectionOriginal(records[2]), false);
expect(hasSelectionOriginal(records[3]), false);
expect(selectionOriginalClipboardText(records, new Set(['failed'])), longOriginal);
expect(
  selectionOriginalClipboardText(
    records,
    new Set(['complete', 'missing', 'legacy', 'voice', 'quickNote', 'failed', 'deleted']),
  ),
  `${longOriginal}\n\n${records[5].rawTranscript}`,
);
expect(selectionOriginalClipboardText(records.slice(1), new Set(['failed'])), '');
expect(
  selectionOriginalClipboardText(
    [{ id: 'spaces', source: 'selection_polish', rawTranscript: '  \t\n ' }],
    new Set(['spaces']),
  ),
  '  \t\n ',
);
