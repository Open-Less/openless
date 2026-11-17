import type { ServiceUsageRecord } from './ipc/service-usage';

export interface ReportedUsageTotals {
  input: number | null;
  output: number | null;
}

function sumKnown(values: Array<number | null>): number | null {
  const known = values.filter(
    (value): value is number => value !== null && Number.isSafeInteger(value) && value >= 0,
  );
  if (!known.length) return null;
  const total = known.reduce((sum, value) => sum + value, 0);
  return Number.isSafeInteger(total) ? total : null;
}

export function summarizeReportedUsage(
  records: ServiceUsageRecord[],
  fromMs: number,
  toMs: number,
) {
  const rows = records.filter(
    (record) =>
      Number.isSafeInteger(record.occurredAtMs) &&
      record.occurredAtMs >= fromMs &&
      record.occurredAtMs <= toMs,
  );
  const summarize = (group: ServiceUsageRecord[]): ReportedUsageTotals => ({
    input: sumKnown(group.map((record) => record.inputTokens)),
    output: sumKnown(group.map((record) => record.outputTokens)),
  });
  const groups = new Map<string, ServiceUsageRecord[]>();
  for (const row of rows) {
    const group = groups.get(row.purpose) ?? [];
    group.push(row);
    groups.set(row.purpose, group);
  }
  return {
    rows,
    totals: summarize(rows),
    byPurpose: [...groups].map(([purpose, group]) => ({ purpose, ...summarize(group) })),
  };
}

export function formatUsageNumber(value: number | null | undefined): string {
  if (value == null || !Number.isSafeInteger(value) || value < 0) return '—';
  if (value < 1_000) return String(value);
  const units = [
    [1_000_000_000, 'B'],
    [1_000_000, 'M'],
    [1_000, 'K'],
  ] as const;
  // Choose the unit after rounding so 999,999 becomes 1M, never 1000K.
  for (let index = 0; index < units.length; index++) {
    const [divisor, suffix] = units[index];
    if (value >= divisor || (index < units.length - 1 && value / divisor >= 0.99995)) {
      return `${Number((value / divisor).toFixed(1))}${suffix}`;
    }
  }
  return String(value);
}

export interface CacheUsageSummary {
  rate: number | null;
  cachedTokens: number | null;
  comparableInputTokens: number | null;
  comparableRecords: number;
  totalRecords: number;
}

function comparableCachePair(record: ServiceUsageRecord): [number, number] | null {
  if (record.kind !== 'llm' && record.kind !== 'multimodal') return null;
  const { inputTokens: input, cachedInputTokens: cached } = record;
  if (
    input == null ||
    cached == null ||
    !Number.isSafeInteger(input) ||
    !Number.isSafeInteger(cached) ||
    input < 0 ||
    cached < 0
  )
    return null;
  let total: number;
  if (record.inputAccounting === 'inclusive') {
    total = input;
  } else if (record.inputAccounting === 'excludesCache') {
    const write = record.cacheWriteInputTokens;
    if (write == null || !Number.isSafeInteger(write) || write < 0) return null;
    total = input + cached + write;
  } else {
    return null;
  }
  return Number.isSafeInteger(total) && total > 0 && cached <= total ? [cached, total] : null;
}

export function summarizeCacheUsage(records: ServiceUsageRecord[]): CacheUsageSummary {
  const pairs = records
    .map(comparableCachePair)
    .filter((pair): pair is [number, number] => pair !== null);
  const cachedTokens = sumKnown(pairs.map(([cached]) => cached));
  const comparableInputTokens = sumKnown(pairs.map(([, input]) => input));
  return {
    rate:
      cachedTokens !== null && comparableInputTokens !== null && comparableInputTokens > 0
        ? cachedTokens / comparableInputTokens
        : null,
    cachedTokens,
    comparableInputTokens,
    comparableRecords: pairs.length,
    totalRecords: records.length,
  };
}

export function reportedCacheHitRate(records: ServiceUsageRecord[]): number | null {
  return summarizeCacheUsage(records).rate;
}

export function summarizeCacheUsageByModel(records: ServiceUsageRecord[]) {
  const groups = new Map<string, ServiceUsageRecord[]>();
  for (const record of records) {
    const key = JSON.stringify([
      record.service ?? null,
      record.channel,
      record.model,
      record.protocol ?? null,
    ]);
    const group = groups.get(key) ?? [];
    group.push(record);
    groups.set(key, group);
  }
  return [...groups]
    .map(([key, group]) => ({
      key,
      service: group[0].service ?? null,
      channel: group[0].channel,
      model: group[0].model,
      protocol: group[0].protocol ?? null,
      ...summarizeCacheUsage(group),
    }))
    .sort((a, b) => b.comparableRecords - a.comparableRecords || b.totalRecords - a.totalRecords);
}
