import type { ActivityDay, DictationSession } from '../types';
import { invokeOrMock } from './shared';
import { previewHistory } from '../../preview/history';

export function listHistory(): Promise<DictationSession[]> {
  return invokeOrMock('list_history', undefined, () => previewHistory.list());
}

/** Daily dictation activity counts (ascending by date), the overview page's yearly heatmap
    data source. Decoupled from the history retention policy. */
export function getActivityStats(): Promise<ActivityDay[]> {
  return invokeOrMock('get_activity_stats', undefined, () => previewHistory.getActivityDays());
}

export function getActivityApps(year: number): Promise<[string, number][]> {
  return invokeOrMock('get_activity_apps', { year }, () => previewHistory.getActivityApps(year));
}

export function deleteHistoryEntry(id: string): Promise<void> {
  return invokeOrMock('delete_history_entry', { id }, () => previewHistory.delete(id));
}

export function clearHistory(): Promise<void> {
  return invokeOrMock('clear_history', undefined, () => previewHistory.clear());
}

/** Reads the data URL (base64) of a session's original microphone WAV.
 *  Call only when session.hasAudioRecording === true, to avoid a useless IPC.
 *  Returns `data:audio/wav;base64,...`, used directly by the frontend `<audio>` and the
 *  export button. */
export function readAudioRecording(sessionId: string): Promise<string> {
  return invokeOrMock('read_audio_recording', { sessionId }, () =>
    previewHistory.readRecording(sessionId),
  );
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
  return invokeOrMock('retranscribe_recording', { sessionId }, () =>
    previewHistory.retranscribe(sessionId),
  );
}

export function applyQuickNoteRepolish(
  sessionId: string,
  text: string,
  stylePackId?: string,
): Promise<DictationSession> {
  return invokeOrMock(
    'apply_quick_note_repolish',
    { sessionId, text, stylePackId: stylePackId ?? null },
    () => previewHistory.repolishQuickNote(sessionId, text, stylePackId),
  );
}
