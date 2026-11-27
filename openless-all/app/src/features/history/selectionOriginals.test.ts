import { hasSelectionOriginal, selectionOriginalClipboardText } from './selectionOriginals';

function expect(actual: unknown, expected: unknown) {
  if (actual !== expected) {
    throw new Error('Selection originals did not preserve record order or verbatim text');
  }
}

const longOriginal = ` \n${'a'.repeat(25_000)}\nMIDDLE-SENTINEL\n${'b'.repeat(25_000)}\t \n`;
const polish = {
  id: 'complete',
  source: 'selection_polish',
  rawTranscript: ' \tSecond original \n',
  finalText: 'Edited result',
} as const;
const voiceEdit = {
  id: 'voice-edit',
  source: 'selection_voice_edit',
  rawTranscript: ' \tVoice edit original\r\nsecond line \n',
  finalText: 'Voice edit result',
} as const;
const missingVoiceEdit = {
  id: 'missing-voice-edit',
  source: 'selection_voice_edit',
  rawTranscript: '',
  finalText: 'Result with no saved original',
} as const;
const records = [
  { id: 'failed', source: 'selection_polish', rawTranscript: longOriginal, errorCode: 'failed' },
  { id: 'voice', source: 'voice', rawTranscript: 'Transcript' },
  { id: 'legacy', rawTranscript: 'Original of unknown source' },
  { id: 'missing', source: 'selection_polish', rawTranscript: '' },
  { id: 'quickNote', source: 'quick_note', rawTranscript: 'Quick note' },
  polish,
  voiceEdit,
  missingVoiceEdit,
] as const;

expect(hasSelectionOriginal(records[0]), true);
expect(hasSelectionOriginal(records[1]), false);
expect(hasSelectionOriginal(records[2]), false);
expect(hasSelectionOriginal(records[3]), false);
expect(hasSelectionOriginal(voiceEdit), true);
expect(hasSelectionOriginal(missingVoiceEdit), false);
expect(selectionOriginalClipboardText(records, new Set(['failed'])), longOriginal);
expect(selectionOriginalClipboardText(records, new Set([voiceEdit.id])), voiceEdit.rawTranscript);
expect(
  selectionOriginalClipboardText(records, new Set([polish.id, 'failed'])),
  `${longOriginal}\n\n${polish.rawTranscript}`,
);
expect(
  selectionOriginalClipboardText(
    records,
    new Set([
      voiceEdit.id,
      missingVoiceEdit.id,
      'missing',
      'legacy',
      'voice',
      'quickNote',
      'failed',
      'deleted',
    ]),
  ),
  `${longOriginal}\n\n${voiceEdit.rawTranscript}`,
);
expect(selectionOriginalClipboardText(records.slice(1), new Set(['failed'])), '');
expect(
  selectionOriginalClipboardText(
    [{ id: 'spaces', source: 'selection_polish', rawTranscript: '  \t\n ' }],
    new Set(['spaces']),
  ),
  '  \t\n ',
);
