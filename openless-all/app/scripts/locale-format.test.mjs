import { register } from 'tsx/esm/api';
register();
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
const { formatHistoryTime, formatLocaleDate, formatLocaleDecimal, formatLocaleNumber } =
  await import('../src/lib/localeFormat.ts');

const westOfUtc = spawnSync(
  process.execPath,
  [
    '--import',
    'tsx',
    '--input-type=module',
    '-e',
    `import { formatLocaleDate } from ${JSON.stringify(new URL('../src/lib/localeFormat.ts', import.meta.url).href)}; console.log(formatLocaleDate('2026-09-08', 'de'));`,
  ],
  { env: { ...process.env, TZ: 'America/Los_Angeles' }, encoding: 'utf8' },
);
assert.equal(westOfUtc.status, 0, westOfUtc.stderr);
assert.equal(westOfUtc.stdout.trim(), '8.9.', 'calendar dates must not shift one day west of UTC');
assert.equal(formatLocaleNumber(12345, 'de'), '12.345');
assert.equal(formatLocaleNumber(12345, 'en'), '12,345');
assert.equal(formatLocaleDecimal(1.5, 'de'), '1,5');
assert.equal(formatLocaleDecimal(1.5, 'en'), '1.5');
const now = new Date(2026, 8, 9, 18, 0);
const today = new Date(2026, 8, 9, 16, 5).toISOString();
const yesterday = new Date(2026, 8, 8, 16, 5).toISOString();
for (const locale of ['zh-CN', 'zh-TW', 'en', 'es', 'fr', 'de', 'ja', 'ko']) {
  assert.equal(
    formatHistoryTime(today, locale, now),
    new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit' }).format(new Date(today)),
  );
  assert.equal(
    formatHistoryTime(yesterday, locale, now),
    new Intl.DateTimeFormat(locale, {
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(yesterday)),
  );
  const day = formatLocaleDate('2026-09-08', locale, {
    timeZone: 'America/Los_Angeles',
    month: 'numeric',
    day: 'numeric',
  });
  // Local calendar keys must be constructed as a local Date before formatting.
  const expected = new Intl.DateTimeFormat(locale, {
    timeZone: 'America/Los_Angeles',
    month: 'numeric',
    day: 'numeric',
  }).format(new Date(2026, 8, 8));
  assert.equal(day, expected);
}
assert.notEqual(formatHistoryTime(yesterday, 'en', now), formatHistoryTime(yesterday, 'de', now));
assert.equal(formatLocaleDate('2026-09-08', 'de'), '8.9.');
assert.equal(formatLocaleDate('not-a-date', 'de'), 'not-a-date');
assert.equal(formatHistoryTime('not-a-date', 'de', now), 'not-a-date');
assert.ok(
  formatHistoryTime(new Date(2025, 8, 8, 16, 5).toISOString(), 'de', now).includes('2025'),
  'old records retain their year',
);
