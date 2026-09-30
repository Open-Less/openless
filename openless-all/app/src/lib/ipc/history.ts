import type { ActivityDay, DictationSession } from '../types';
import { invokeOrMock } from './shared';
import { mockActivityDays, mockHistory } from './mock-data';

export function listHistory(): Promise<DictationSession[]> {
  return invokeOrMock('list_history', undefined, () => mockHistory);
}

/** Daily dictation activity counts (ascending by date), the overview page's yearly heatmap
    data source. Decoupled from the history retention policy. */
export function getActivityStats(): Promise<ActivityDay[]> {
  return invokeOrMock('get_activity_stats', undefined, () => mockActivityDays);
}

export function deleteHistoryEntry(id: string): Promise<void> {
  return invokeOrMock('delete_history_entry', { id }, () => undefined);
}

export function clearHistory(): Promise<void> {
  return invokeOrMock('clear_history', undefined, () => undefined);
}

/** Reads the data URL (base64) of a session's original microphone WAV.
 *  Call only when session.hasAudioRecording === true, to avoid a useless IPC.
 *  Returns `data:audio/wav;base64,...`, used directly by the frontend `<audio>` and the
 *  export button. */
export function readAudioRecording(sessionId: string): Promise<string> {
  return invokeOrMock('read_audio_recording', { sessionId }, () => 'data:audio/wav;base64,');
}

export interface HistoryRetranscriptionResult {
  text: string;
  updatedEntry: DictationSession | null;
}

/** Re-transcribes a history entry with an archived recording using the current ASR provider
 *  (issue #613 / #1046).
 *  Transcription-failed entries get repaired; completed / polish-failed entries only get a
 *  transient result, never overwriting the original history.
 *  Throws on failure (e.g. "retranscription still found no speech" / "recording not
 *  found"); the recording is kept, never lost.
 *  Callable on successful, polish-failed, and transcription-failed entries; the backend
 *  re-validates the recording and capability boundaries. */
export function retranscribeRecording(sessionId: string): Promise<HistoryRetranscriptionResult> {
  return invokeOrMock('retranscribe_recording', { sessionId }, () => ({
    text: mockHistory[0].rawTranscript,
    updatedEntry: null,
  })) as Promise<HistoryRetranscriptionResult>;
}

export function applyQuickNoteRepolish(
  sessionId: string,
  text: string,
  stylePackId?: string,
): Promise<DictationSession> {
  return invokeOrMock(
    'apply_quick_note_repolish',
    { sessionId, text, stylePackId: stylePackId ?? null },
    () => mockHistory[0],
  ) as Promise<DictationSession>;
}
