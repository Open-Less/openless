import { canRetranscribeHistoryEntry, retranscriptionErrorKey } from './history-retranscribe';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const archivedEntry = {
  hasAudioRecording: true,
  pipelineMode: 'traditional',
} as const;

const entryWithError = (errorCode: string | null) => ({ ...archivedEntry, errorCode });

assert(
  canRetranscribeHistoryEntry(entryWithError(null)),
  'completed entries with an archived recording should be retranscribable',
);
assert(
  canRetranscribeHistoryEntry(entryWithError('polishFailed')),
  'entries whose polishing failed should still be retranscribable',
);
assert(
  canRetranscribeHistoryEntry(entryWithError('transcribeFailed')),
  'entries whose transcription failed should be retranscribable',
);
assert(
  !canRetranscribeHistoryEntry({
    hasAudioRecording: false,
    pipelineMode: 'traditional',
  }),
  'entries without an archived recording should not show retranscription',
);
assert(
  !canRetranscribeHistoryEntry({
    hasAudioRecording: null,
    pipelineMode: undefined,
  }),
  'legacy entries without recording metadata should not show retranscription',
);
assert(
  !canRetranscribeHistoryEntry({
    ...archivedEntry,
    pipelineMode: 'multimodal',
  }),
  'multimodal entries should not show an unsupported retranscription action',
);
assert(
  canRetranscribeHistoryEntry({
    ...archivedEntry,
    pipelineMode: 'multimodal',
    source: 'quick_note',
  }),
  'quick notes should retain retranscription even when their original pipeline was multimodal',
);

assert(
  canRetranscribeHistoryEntry({ ...archivedEntry, pipelineMode: 'multimodal' }, true),
  'Linux must offer ASR recovery for an archived failed multimodal request',
);
assert(
  !canRetranscribeHistoryEntry({ hasAudioRecording: false, pipelineMode: 'multimodal' }, true),
  'Linux recovery still requires an archived recording',
);

console.log('history-retranscribe: all assertions passed');

assert(
  retranscriptionErrorKey('historyRetranscribeNoSpeech') === 'history.retranscribeErrors.NoSpeech',
  'empty ASR must retain its reason',
);
assert(
  retranscriptionErrorKey('raw body with a secret') === 'history.retranscribeErrors.Provider',
  'provider bodies must not reach UI',
);
