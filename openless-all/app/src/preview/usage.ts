import type { ServiceUsageRecord, ServiceUsageSnapshot } from '../lib/ipc/service-usage';

const now = Date.now();
let records: ServiceUsageRecord[] = [
  {
    id: 'preview-asr',
    occurredAtMs: now,
    kind: 'asr',
    purpose: 'dictation',
    outcome: 'succeeded',
    provider: 'openrouter',
    channel: null,
    model: 'whisper',
    audioMs: 4500,
    inputTokens: null,
    outputTokens: null,
    cachedInputTokens: null,
    cacheWriteInputTokens: null,
    reasoningTokens: null,
    synthesizedCharacters: null,
  },
  {
    id: 'preview-polish',
    occurredAtMs: now,
    kind: 'llm',
    purpose: 'dictationPostprocess',
    outcome: 'succeeded',
    provider: 'ark',
    channel: null,
    model: 'deepseek-v3-2',
    audioMs: null,
    inputTokens: 118,
    outputTokens: 42,
    cachedInputTokens: null,
    cacheWriteInputTokens: null,
    reasoningTokens: null,
    synthesizedCharacters: null,
  },
];

export function getPreviewServiceUsage(): ServiceUsageSnapshot {
  return { records: records.map((row) => ({ ...row })), droppedOrFailed: 0 };
}

export function clearPreviewServiceUsage(): void {
  records = [];
}
