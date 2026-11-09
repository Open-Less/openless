import { invokeOrMock } from './shared';

export interface ServiceUsageRecord {
  id: string;
  occurredAtMs: number;
  kind: 'asr' | 'llm' | 'tts' | 'multimodal';
  purpose:
    | 'dictation'
    | 'dictationPostprocess'
    | 'translation'
    | 'question'
    | 'selection'
    | 'quickNote'
    | 'voiceEdit'
    | 'cloudNote'
    | 'other';
  outcome: 'succeeded' | 'failed' | 'cancelled' | 'unknown';
  provider: string | null;
  channel: string | null;
  model: string | null;
  audioMs: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cachedInputTokens: number | null;
  cacheWriteInputTokens: number | null;
  reasoningTokens: number | null;
  synthesizedCharacters: number | null;
}
export interface ServiceUsageSnapshot {
  records: ServiceUsageRecord[];
  droppedOrFailed: number;
}
export function getServiceUsage(): Promise<ServiceUsageSnapshot> {
  return invokeOrMock('get_service_usage', undefined, () => ({ records: [], droppedOrFailed: 0 }));
}
export function clearServiceUsage(): Promise<void> {
  return invokeOrMock('clear_service_usage', undefined, () => undefined);
}
