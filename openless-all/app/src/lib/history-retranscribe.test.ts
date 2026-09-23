import { canRetranscribeHistoryEntry } from './history-retranscribe';

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

console.log('history-retranscribe: all assertions passed');
