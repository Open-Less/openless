// 概览页「近 7 天 / 近 30 天」周期指标的聚合。
//
// 数据源是 activity 存储（date → {count, chars, durationMs}），**不是** listHistory()：
// 历史受 200 条上限约束，日均上百次的用户几天就把上周挤没了，按历史现算会把没数据的
// 那几天画成 0（明明年度热力图上是亮的）。activity 保留两年且只存聚合数字。

import type { ActivityDay } from './types';

export const ACTIVITY_PERIODS = [7, 30] as const;
export type ActivityPeriod = (typeof ACTIVITY_PERIODS)[number];

export const ACTIVITY_METRICS = ['count', 'chars', 'duration'] as const;
export type ActivityMetric = (typeof ACTIVITY_METRICS)[number];

export interface ActivityBucket {
  /** 本地日期 YYYY-MM-DD，与后端 chrono::Local 写入的键同格式。 */
  date: string;
  value: number;
}

export interface PeriodSeries {
  /** 长度恒等于 days，按日期升序，最后一个是今天。缺数据的日期补 0。 */
  buckets: ActivityBucket[];
  total: number;
  /** 周期内日均值。分母是整个周期（含没说话的日子），不是「有记录的天数」。 */
  dailyAverage: number;
}

/** 本地日期键。必须用本地年月日拼，不能用 toISOString()——后者按 UTC 切日，
 *  东八区凌晨的会话会被算到前一天，与后端 chrono::Local 的键对不上。 */
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
 * 把活动快照裁成「今天往前数 days 天」的连续序列。
 *
 * 老数据（升级前写入的裸数字）没有 chars / durationMs，读回是 0：这些天在字数/时长
 * 指标里显示为 0 是诚实的——当时确实没记，不该凭历史现算去伪造一个受 200 条上限
 * 影响的数字。条数指标不受影响，全程可用。
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
