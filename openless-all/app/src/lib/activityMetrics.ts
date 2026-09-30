// Aggregation for the overview page's "last 7 days / last 30 days" period metrics.
//
// The data source is the activity store (date → {count, chars, durationMs}), **not**
// listHistory(): history is capped at 200 entries, so a user with hundreds of sessions a
// day pushes last week out within days; computing from history would draw the days without
// data as 0 (even though the yearly heatmap lights up). Activity is kept for two years and
// stores only aggregate numbers.

import type { ActivityDay } from './types';

export const ACTIVITY_PERIODS = [7, 30] as const;
export type ActivityPeriod = (typeof ACTIVITY_PERIODS)[number];

export const ACTIVITY_METRICS = ['count', 'chars', 'duration'] as const;
export type ActivityMetric = (typeof ACTIVITY_METRICS)[number];

export interface ActivityBucket {
  /** Local date YYYY-MM-DD, same format as the keys written by the backend's chrono::Local. */
  date: string;
  value: number;
}

export interface PeriodSeries {
  /** Length always equals days, ascending by date, last one is today. Days without data are filled with 0. */
  buckets: ActivityBucket[];
  total: number;
  /** Daily average over the period. The denominator is the whole period (including silent
      days), not "days with records". */
  dailyAverage: number;
}

/** Local date key. Must be built from local year/month/day, not toISOString() — the latter
 *  slices days by UTC, so a session just after midnight in UTC+8 would land on the previous
 *  day and mismatch the backend's chrono::Local keys. */
export function localDateKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function readMetric(day: ActivityDay, metric: ActivityMetric): number {
  switch (metric) {
    case 'count':
      return day.count;
    case 'chars':
      return day.chars ?? 0;
    case 'duration':
      return day.durationMs ?? 0;
  }
}

/**
 * Trims the activity snapshot into a continuous series of the last `days` days ending today.
 *
 * Old data (bare numbers written before the upgrade) lacks chars / durationMs and reads
 * back as 0: showing 0 for those days in the chars/duration metrics is honest — nothing was
 * recorded then, and it must not be fabricated from history, which is capped at 200
 * entries. The count metric is unaffected and works for the full range.
 */
export function buildPeriodSeries(
  activity: readonly ActivityDay[],
  days: number,
  metric: ActivityMetric,
  today: Date = new Date(),
): PeriodSeries {
  const byDate = new Map<string, ActivityDay>();
  for (const day of activity) byDate.set(day.date, day);

  const buckets: ActivityBucket[] = [];
  let total = 0;
  for (let offset = days - 1; offset >= 0; offset--) {
    const date = new Date(today);
    date.setHours(0, 0, 0, 0);
    date.setDate(date.getDate() - offset);
    const key = localDateKey(date);
    const day = byDate.get(key);
    const value = day ? readMetric(day, metric) : 0;
    total += value;
    buckets.push({ date: key, value });
  }

  return { buckets, total, dailyAverage: days > 0 ? total / days : 0 };
}
