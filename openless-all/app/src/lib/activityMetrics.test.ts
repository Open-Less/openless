import { buildPeriodSeries, localDateKey } from './activityMetrics';
import type { ActivityDay } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

function day(date: string, count: number, chars: number, durationMs: number): ActivityDay {
  return { date, count, chars, durationMs };
}

// Local date keys must be built from local year/month/day. In UTC+8, toISOString() at early
// morning switches to the previous day and mismatches the keys the backend writes with
// chrono::Local, making the whole series read as 0.
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

// 7-day window: length always 7, ascending by date, last is today, missing dates filled with 0.
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

// Metric switching reads different fields; window logic unchanged.
const weekChars = buildPeriodSeries(activity, 7, 'chars', today);
assert(
  weekChars.total === 4000 + 11_800 + 4400 + 3200,
  `7-day chars total wrong: ${weekChars.total}`,
);
const weekDuration = buildPeriodSeries(activity, 7, 'duration', today);
assert(
  weekDuration.total === 400_000 + 1_180_000 + 440_000 + 320_000,
  `7-day duration total wrong: ${weekDuration.total}`,
);

// The 30-day window includes earlier dates (everything from 07-29 here falls inside it); length always 30.
const month = buildPeriodSeries(activity, 30, 'count', today);
assert(
  month.buckets.length === 30,
  `30-day window should have 30 buckets, got ${month.buckets.length}`,
);
assert(
  month.buckets[29].date === '2026-08-04' && month.buckets[0].date === '2026-07-06',
  `30-day window should span 07-06..08-04, got ${month.buckets[0].date}..${month.buckets[29].date}`,
);
assert(month.total === 234, `30-day count total wrong: ${month.total}`);

// Pre-upgrade data has only count, no chars / durationMs. Chars/duration read as 0 and must not
// be NaN — NaN would break the whole bar chart's max computation.
const legacy: ActivityDay[] = [{ date: '2026-08-03', count: 156 } as ActivityDay];
const legacyChars = buildPeriodSeries(legacy, 7, 'chars', today);
assert(legacyChars.total === 0, `legacy entries should read as 0 chars, got ${legacyChars.total}`);
assert(Number.isFinite(legacyChars.dailyAverage), 'legacy entries must not produce NaN averages');
const legacyCount = buildPeriodSeries(legacy, 7, 'count', today);
assert(legacyCount.total === 156, 'legacy entries should still report their count');

// An empty dataset must not blow up; all zeros.
const empty = buildPeriodSeries([], 7, 'count', today);
assert(
  empty.buckets.length === 7 && empty.total === 0 && empty.dailyAverage === 0,
  'empty activity should yield a zeroed series',
);

// Cross-month boundary: the window must correctly reach back into the previous month, not truncate at the 1st.
const firstOfMonth = new Date(2026, 7, 1, 9, 0, 0); // 2026-08-01
const crossMonth = buildPeriodSeries(activity, 7, 'count', firstOfMonth);
assert(
  crossMonth.buckets[0].date === '2026-07-26' && crossMonth.buckets[6].date === '2026-08-01',
  `window should cross the month boundary, got ${crossMonth.buckets[0].date}..${crossMonth.buckets[6].date}`,
);
assert(crossMonth.total === 40 + 118, `cross-month total wrong: ${crossMonth.total}`);
