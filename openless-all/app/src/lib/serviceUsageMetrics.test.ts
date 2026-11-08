import { summarizeReportedUsage } from './serviceUsageMetrics';
import type { ServiceUsageRecord } from './ipc/service-usage';

function equal(actual: unknown, expected: unknown, label: string) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(label);
}
const make = (overrides: Partial<ServiceUsageRecord>): ServiceUsageRecord => ({
  id: 'id',
  occurredAtMs: 100,
  kind: 'llm',
  purpose: 'dictationPostprocess',
  outcome: 'unknown',
  provider: 'example',
  channel: 'channel',
  model: 'model',
  audioMs: null,
  inputTokens: null,
  outputTokens: null,
  cachedInputTokens: null,
  cacheWriteInputTokens: null,
  reasoningTokens: null,
  synthesizedCharacters: null,
  ...overrides,
});
const summary = summarizeReportedUsage(
  [
    make({ id: 'a', inputTokens: 50, outputTokens: 0, cachedInputTokens: 20 }),
    make({ id: 'b', purpose: 'translation', inputTokens: null, outputTokens: 7 }),
    make({ id: 'c', occurredAtMs: 99, inputTokens: 999 }),
    make({ id: 'd', occurredAtMs: 201, inputTokens: 999 }),
  ],
  100,
  200,
);
equal(summary.totals, { input: 50, output: 7 }, 'totals use reported fields in range');
equal(
  summary.byPurpose,
  [
    { purpose: 'dictationPostprocess', input: 50, output: 0 },
    { purpose: 'translation', input: null, output: 7 },
  ],
  'usage is grouped without adding cache counts',
);
equal(
  summarizeReportedUsage([], 0, 200).totals,
  { input: null, output: null },
  'missing is unknown',
);
equal(
  summarizeReportedUsage(
    [make({ inputTokens: Number.MAX_SAFE_INTEGER }), make({ inputTokens: 1 })],
    0,
    200,
  ).totals.input,
  null,
  'unsafe sums are unknown',
);
console.log('reported service usage metrics tests passed');
