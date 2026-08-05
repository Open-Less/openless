import { buildPeriodSeries, localDateKey } from './activityMetrics';
import type { ActivityDay } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

function day(date: string, count: number, chars: number, durationMs: number): ActivityDay {
  return { date, count, chars, durationMs };
}

// 本地日期键必须按本地年月日拼。东八区凌晨用 toISOString() 会切到前一天，
// 与后端 chrono::Local 写入的键对不上，整段数据会读成 0。
const localMidnight = new Date(2026, 7, 4, 0, 30, 0);
assert(
  localDateKey(localMidnight) === '2026-08-04',
  `local date key should follow local calendar day, got ${localDateKey(localMidnight)}`,
);

const today = new Date(2026, 7, 4, 12, 0, 0); // 2026-08-04
const activity: ActivityDay[] = [
  day('2026-07-29', 40, 4000, 400_000),
  day('2026-07-30', 118, 11_800, 1_180_000),
  day('2026-08-02', 44, 4400, 440_000),
  day('2026-08-04', 32, 3200, 320_000),
];

// 7 天窗口：长度恒为 7、按日期升序、最后一个是今天、缺失日期补 0。
const week = buildPeriodSeries(activity, 7, 'count', today);
assert(week.buckets.length === 7, `7-day window should have 7 buckets, got ${week.buckets.length}`);
assert(
  week.buckets[0].date === '2026-07-29' && week.buckets[6].date === '2026-08-04',
  `window should span 07-29..08-04, got ${week.buckets[0].date}..${week.buckets[6].date}`,
);
assert(
  week.buckets[2].date === '2026-07-31' && week.buckets[2].value === 0,
  'a date with no activity should be a zero bucket, not a gap',
);
assert(week.total === 40 + 118 + 44 + 32, `7-day count total wrong: ${week.total}`);
assert(
  Math.abs(week.dailyAverage - 234 / 7) < 1e-9,
  `daily average should divide by the whole period, got ${week.dailyAverage}`,
);

// 指标切换读的是不同字段，窗口逻辑不变。
const weekChars = buildPeriodSeries(activity, 7, 'chars', today);
assert(weekChars.total === 4000 + 11_800 + 4400 + 3200, `7-day chars total wrong: ${weekChars.total}`);
const weekDuration = buildPeriodSeries(activity, 7, 'duration', today);
assert(
  weekDuration.total === 400_000 + 1_180_000 + 440_000 + 320_000,
  `7-day duration total wrong: ${weekDuration.total}`,
);

// 30 天窗口把更早的日期也纳进来（这里 07-29 起的都在窗口内），长度恒为 30。
const month = buildPeriodSeries(activity, 30, 'count', today);
assert(month.buckets.length === 30, `30-day window should have 30 buckets, got ${month.buckets.length}`);
assert(
  month.buckets[29].date === '2026-08-04' && month.buckets[0].date === '2026-07-06',
  `30-day window should span 07-06..08-04, got ${month.buckets[0].date}..${month.buckets[29].date}`,
);
assert(month.total === 234, `30-day count total wrong: ${month.total}`);

// 升级前写入的老数据只有 count，没有 chars / durationMs。字数/时长按 0 读，
// 不能 NaN —— NaN 会把整个柱状图的 max 算坏。
const legacy: ActivityDay[] = [{ date: '2026-08-03', count: 156 } as ActivityDay];
const legacyChars = buildPeriodSeries(legacy, 7, 'chars', today);
assert(legacyChars.total === 0, `legacy entries should read as 0 chars, got ${legacyChars.total}`);
assert(
  Number.isFinite(legacyChars.dailyAverage),
  'legacy entries must not produce NaN averages',
);
const legacyCount = buildPeriodSeries(legacy, 7, 'count', today);
assert(legacyCount.total === 156, 'legacy entries should still report their count');

// 空数据集不炸，全 0。
const empty = buildPeriodSeries([], 7, 'count', today);
assert(empty.buckets.length === 7 && empty.total === 0 && empty.dailyAverage === 0, 'empty activity should yield a zeroed series');

// 跨月边界：窗口要正确回退到上个月，不能在 1 号截断。
const firstOfMonth = new Date(2026, 7, 1, 9, 0, 0); // 2026-08-01
const crossMonth = buildPeriodSeries(activity, 7, 'count', firstOfMonth);
assert(
  crossMonth.buckets[0].date === '2026-07-26' && crossMonth.buckets[6].date === '2026-08-01',
  `window should cross the month boundary, got ${crossMonth.buckets[0].date}..${crossMonth.buckets[6].date}`,
);
assert(crossMonth.total === 40 + 118, `cross-month total wrong: ${crossMonth.total}`);
