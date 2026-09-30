import type { DictationSession } from './types';

/**
 * Re-transcription requires a still-existing WAV archive. Successful transcription,
 * polish-failed, transcription-failed, and quick-note entries can all re-validate the
 * current ASR provider against the same audio. Whether a failed record gets rewritten is
 * decided by the backend.
 */
export function canRetranscribeHistoryEntry(
  session: Pick<DictationSession, 'hasAudioRecording' | 'pipelineMode' | 'source'>,
): boolean {
  return (
    session.hasAudioRecording === true &&
    (session.pipelineMode !== 'multimodal' || session.source === 'quick_note')
  );
}
