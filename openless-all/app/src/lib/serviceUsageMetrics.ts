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
