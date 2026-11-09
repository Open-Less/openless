import {
  summarizeReportedUsage,
  formatUsageNumber,
  reportedCacheHitRate,
  summarizeCacheUsage,
  summarizeCacheUsageByModel,
} from './serviceUsageMetrics';
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

for (const [value, expected] of [
  [null, '—'],
  [-1, '—'],
  [999, '999'],
  [1000, '1K'],
  [12500, '12.5K'],
  [999999, '1M'],
  [1250000, '1.3M'],
  [2300000000, '2.3B'],
  [Number.MAX_SAFE_INTEGER + 1, '—'],
] as const) {
  equal(formatUsageNumber(value), expected, `compact number ${value}`);
}
const inclusive = (overrides: Partial<ServiceUsageRecord>) =>
  make({ inputAccounting: 'inclusive', ...overrides });
const additive = (overrides: Partial<ServiceUsageRecord>) =>
  make({ inputAccounting: 'excludesCache', ...overrides });
equal(
  reportedCacheHitRate([
    inclusive({ provider: 'unknown-gateway', inputTokens: 100, cachedInputTokens: 80 }),
    inclusive({ provider: 'openai', inputTokens: 300, cachedInputTokens: 0 }),
  ]),
  0.2,
  'canonical field accounting replaces the provider-name whitelist',
);
equal(
  reportedCacheHitRate([
    additive({ inputTokens: 10, cachedInputTokens: 80, cacheWriteInputTokens: 10 }),
  ]),
  0.8,
  'Messages denominator includes raw input, cache read and creation once',
);
equal(
  reportedCacheHitRate([
    additive({ inputTokens: 10, cachedInputTokens: 0, cacheWriteInputTokens: 0 }),
  ]),
  0,
  'reported zero cache is a valid zero rate',
);
for (const record of [
  make({ provider: 'openai', inputTokens: 100, cachedInputTokens: 80 }),
  inclusive({ inputTokens: 100, cachedInputTokens: null }),
  inclusive({ inputTokens: 0, cachedInputTokens: 0 }),
  inclusive({ inputTokens: 100, cachedInputTokens: 101 }),
  additive({ inputTokens: 100, cachedInputTokens: 80, cacheWriteInputTokens: null }),
  additive({
    inputTokens: Number.MAX_SAFE_INTEGER,
    cachedInputTokens: 1,
    cacheWriteInputTokens: 0,
  }),
  inclusive({ inputTokens: -1, cachedInputTokens: 0 }),
])
  equal(
    reportedCacheHitRate([record]),
    null,
    'legacy, incomplete and unsafe quantities remain unknown',
  );
equal(
  reportedCacheHitRate([
    inclusive({ inputTokens: Number.MAX_SAFE_INTEGER, cachedInputTokens: 0 }),
    inclusive({ inputTokens: 1, cachedInputTokens: 0 }),
  ]),
  null,
  'aggregate overflow remains unknown',
);
const cohorts = [
  inclusive({
    service: 'deepseek',
    channel: 'direct',
    model: 'v4',
    protocol: 'chat_completions',
    inputTokens: 100,
    cachedInputTokens: 80,
  }),
  inclusive({
    service: 'deepseek',
    channel: 'direct',
    model: 'v4',
    protocol: 'chat_completions',
    inputTokens: 100,
    cachedInputTokens: 20,
  }),
  make({
    service: 'deepseek',
    channel: 'direct',
    model: 'v4',
    protocol: 'chat_completions',
    inputTokens: 100,
    cachedInputTokens: 99,
  }),
  inclusive({
    service: 'ark',
    channel: 'hosted',
    model: 'v4',
    protocol: 'chat_completions',
    inputTokens: 100,
    cachedInputTokens: 10,
  }),
  additive({
    service: 'ark',
    channel: 'hosted',
    model: 'v4',
    protocol: 'messages',
    inputTokens: 10,
    cachedInputTokens: 70,
    cacheWriteInputTokens: 20,
  }),
];
const before = JSON.stringify(cohorts);
const groups = summarizeCacheUsageByModel(cohorts);
equal(groups.length, 3, 'service, channel and protocol distinguish identical model names');
equal(groups[0].rate, 0.5, 'rates are token-weighted within their own cohort');
equal(groups[0].comparableRecords, 2, 'comparable report count is explicit');
equal(groups[0].totalRecords, 3, 'unclassified legacy reports remain in coverage denominator');
equal(summarizeCacheUsage([]).rate, null, 'empty rate is unknown');
equal(JSON.stringify(cohorts), before, 'normalization never rewrites raw quantities');
console.log('compact usage numbers and protocol-aware cache rate tests passed');

equal(
  reportedCacheHitRate([
    inclusive({ inputTokens: 100, cachedInputTokens: 30, cacheWriteInputTokens: 20 }),
  ]),
  0.3,
  'inclusive cache writes are already counted in input',
);
