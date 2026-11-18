import { isTauriHost } from '../runtime/environment';
import { invokeOrPreview } from '../runtime/transport';
import { downloadUrl } from '../runtime/files';
import type { ActivityDay, DictationSession } from './contracts';
import type { QuerySnapshot } from '../data/snapshot-query';
import { historyStore } from '../data/history';
import * as commands from '../lib/ipc/history';

export { getActivityApps, type YearlyActivityApps } from '../data/history';
export { readAudioRecording, type HistoryRetranscriptionResult } from '../lib/ipc/history';

export function deleteHistoryEntry(id: string): Promise<void> {
  return historyStore.mutate(() => commands.deleteHistoryEntry(id));
}

export function clearHistory(): Promise<void> {
  return historyStore.mutate(commands.clearHistory);
}

export function retranscribeRecording(sessionId: string) {
  return historyStore.mutate(
    () => commands.retranscribeRecording(sessionId),
    (result) => result.updatedEntry !== null,
  );
}

export function applyQuickNoteRepolish(sessionId: string, text: string, stylePackId?: string) {
  return historyStore.mutate(() => commands.applyQuickNoteRepolish(sessionId, text, stylePackId));
}

function readData<T>({ data, error }: QuerySnapshot<T>): T {
  if (error !== null) throw error;
  if (data === null) throw new Error('History snapshot is unavailable');
  return data;
}

export async function listHistory(): Promise<DictationSession[]> {
  return readData(await historyStore.records.load());
}

export async function getActivityStats(): Promise<ActivityDay[]> {
  return readData(await historyStore.activity.load());
}

export async function exportAudioRecording(sessionId: string): Promise<void> {
  if (isTauriHost()) {
    return invokeOrPreview('export_audio_recording', { sessionId }, () => undefined);
  }
  const dataUrl = await commands.readAudioRecording(sessionId);
  if (!dataUrl.startsWith('data:audio/wav;base64,') || dataUrl.endsWith(',')) {
    throw new Error('empty recording');
  }
  downloadUrl(dataUrl, `openless-recording-${sessionId}.wav`);
}
