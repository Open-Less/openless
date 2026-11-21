import { invokeOrMock } from './shared';
import { isHarmonyRuntime } from '../../runtime/environment';
import { getPreviewServiceUsage, clearPreviewServiceUsage } from '../../preview/usage';

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
  service?: string | null;
  protocol?: string | null;
  inputAccounting?: 'inclusive' | 'excludesCache' | null;
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
export function supportsServiceUsage(): boolean {
  return !isHarmonyRuntime();
}

export async function getServiceUsage(): Promise<ServiceUsageSnapshot> {
  if (!supportsServiceUsage()) throw new Error('service usage is unavailable');
  return invokeOrMock('get_service_usage', undefined, getPreviewServiceUsage);
}

export async function clearServiceUsage(): Promise<void> {
  if (!supportsServiceUsage()) throw new Error('service usage is unavailable');
  return invokeOrMock('clear_service_usage', undefined, clearPreviewServiceUsage);
}
