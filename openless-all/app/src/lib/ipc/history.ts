import type { ActivityDay, DictationSession } from '../types';
import { invokeOrMock } from './shared';
import { mockActivityDays, mockHistory } from './mock-data';

export function listHistory(): Promise<DictationSession[]> {
  return invokeOrMock('list_history', undefined, () => mockHistory);
}

/** 每日听写活动计数（日期升序），概览页年度热力图数据源。与历史保留策略解耦。 */
export function getActivityStats(): Promise<ActivityDay[]> {
  return invokeOrMock('get_activity_stats', undefined, () => mockActivityDays);
}

export function deleteHistoryEntry(id: string): Promise<void> {
  return invokeOrMock('delete_history_entry', { id }, () => undefined);
}

export function clearHistory(): Promise<void> {
  return invokeOrMock('clear_history', undefined, () => undefined);
}

/** 读取某次会话的原始麦克风 WAV 的 data URL（base64）。
 *  仅当 session.hasAudioRecording === true 时调用，避免无效 IPC。
 *  返回 `data:audio/wav;base64,...` 格式，前端 `<audio>` 和导出按钮直接使用。 */
export function readAudioRecording(sessionId: string): Promise<string> {
  return invokeOrMock('read_audio_recording', { sessionId }, () => 'data:audio/wav;base64,');
}

export interface HistoryRetranscriptionResult {
  text: string;
  updatedEntry: DictationSession | null;
}

/** 用当前 ASR provider 对一条有归档录音的历史条目重新转录（issue #613 / #1046）。
 *  转录失败记录会被修复；已完成 / 润色失败记录只返回临时结果，不覆盖原历史。
 *  失败时抛出错误（如「重新转录仍未识别到语音」/「recording not found」），录音保留不丢。
 *  成功、润色失败和转录失败的条目都可调用，后端再次校验录音与能力边界。 */
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
