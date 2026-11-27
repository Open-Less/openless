import type { DictationSession } from './types';
import { isLinuxHost } from '../runtime/environment';

/**
 * Re-transcription requires a still-existing WAV archive. Successful transcription,
 * polish-failed, transcription-failed, and quick-note entries can all re-validate the
 * current ASR provider against the same audio. Whether a failed record gets rewritten is
 * decided by the backend.
 */
export function canRetranscribeHistoryEntry(
  session: Pick<DictationSession, 'hasAudioRecording' | 'pipelineMode' | 'source'>,
  allowMultimodalRecovery = isLinuxHost(),
): boolean {
  // Recovery uses the currently configured ASR provider, independently of the
  // original pipeline. A timed-out multimodal request must not strand its WAV.
  return session.hasAudioRecording === true &&
    (session.pipelineMode !== 'multimodal' || session.source === 'quick_note' || allowMultimodalRecovery);
}

const retranscriptionReasons = new Set([
  'NoSpeech',
  'Missing',
  'InvalidAudio',
  'Permission',
  'Active',
  'Busy',
  'Unsupported',
  'Configuration',
  'Storage',
  'Provider',
  'InvalidId',
]);

/** Only the host's bounded error categories are safe to show as reasons. */
export function retranscriptionErrorKey(message: string): string {
  const reason = message.replace(/^historyRetranscribe/, '');
  if (message.startsWith('historyRetranscribe') && retranscriptionReasons.has(reason)) {
    return `history.retranscribeErrors.${reason}`;
  }
  // Older hosts returned this fixed message instead of a category.
  if (message === '重新转录仍未识别到语音') return 'history.retranscribeErrors.NoSpeech';
  if (message === 'recording not found') return 'history.retranscribeErrors.Missing';
  return 'history.retranscribeErrors.Provider';
}
