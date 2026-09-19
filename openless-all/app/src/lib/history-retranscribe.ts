import type { DictationSession } from './types';

/**
 * 重新转录需要一份仍存在的 WAV 归档。成功转录、润色失败、转录失败和速记
 * 都可以用同一份音频重新验证当前 ASR provider。是否回写失败记录由后端决定。
 */
export function canRetranscribeHistoryEntry(
  session: Pick<DictationSession, 'hasAudioRecording' | 'pipelineMode' | 'source'>,
): boolean {
  return (
    session.hasAudioRecording === true &&
    (session.pipelineMode !== 'multimodal' || session.source === 'quick_note')
  );
}
