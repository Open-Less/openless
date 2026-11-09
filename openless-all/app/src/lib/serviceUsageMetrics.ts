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

export function reportedCacheHitRate(records: ServiceUsageRecord[]): number | null {
  const compatible = records.filter(
    (record) =>
      ['openai', 'deepseek'].includes(record.provider?.toLowerCase() ?? '') &&
      record.cacheWriteInputTokens == null &&
      Number.isSafeInteger(record.inputTokens) &&
      record.inputTokens! > 0 &&
      Number.isSafeInteger(record.cachedInputTokens) &&
      record.cachedInputTokens! >= 0 &&
      record.cachedInputTokens! <= record.inputTokens!,
  );
  if (!compatible.length) return null;
  const input = sumKnown(compatible.map((record) => record.inputTokens));
  const cached = sumKnown(compatible.map((record) => record.cachedInputTokens));
  return input !== null && input > 0 && cached !== null ? cached / input : null;
}
