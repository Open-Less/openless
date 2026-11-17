import {
  RECORDING_COUNTDOWN_URGENT_SECONDS,
  formatRecordingCountdown,
  recordingCountdownUrgent,
} from './recordingCountdown.ts';
import { zhCN } from '../i18n/zh-CN';
import { zhTW } from '../i18n/zh-TW';
import { en } from '../i18n/en';
import { ja } from '../i18n/ja';
import { ko } from '../i18n/ko';
import { es } from '../i18n/es';
import { fr } from '../i18n/fr';
import { de } from '../i18n/de';

function assertEqual<T>(actual: T, expected: T, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

// The backend publishes nothing until the final minute, so "no payload" must render nothing.
assertEqual(formatRecordingCountdown(undefined), null, 'missing countdown renders nothing');
assertEqual(formatRecordingCountdown(null), null, 'null countdown renders nothing');
assertEqual(formatRecordingCountdown(Number.NaN), null, 'NaN countdown renders nothing');
assertEqual(formatRecordingCountdown(-1), null, 'negative countdown renders nothing');

assertEqual(formatRecordingCountdown(60), '1:00', 'a full minute reads as 1:00');
assertEqual(formatRecordingCountdown(59), '0:59', 'the last minute is padded');
assertEqual(formatRecordingCountdown(9), '0:09', 'single-digit seconds stay two digits');
assertEqual(formatRecordingCountdown(1), '0:01', 'the final second is visible');
assertEqual(formatRecordingCountdown(0), '0:00', 'zero is still a valid frame before the stop lands');

// The backend rounds up, so a fractional value must not be floored into the previous second.
assertEqual(formatRecordingCountdown(42.9), '0:42', 'fractional seconds truncate for display only');

assertEqual(
  RECORDING_COUNTDOWN_URGENT_SECONDS,
  10,
  'the urgent window stays at the last ten seconds',
);
assertEqual(recordingCountdownUrgent(11), false, 'eleven seconds is not urgent yet');
assertEqual(recordingCountdownUrgent(10), true, 'ten seconds turns the badge urgent');
assertEqual(recordingCountdownUrgent(0), true, 'the last second stays urgent');
assertEqual(recordingCountdownUrgent(undefined), false, 'no countdown is never urgent');
assertEqual(recordingCountdownUrgent(Number.NaN), false, 'NaN is never urgent');

// The badge's screen-reader label is translated, so every shipped locale must carry the
// key with the same {{time}} placeholder — otherwise a non-English user hears a raw key.
for (const [locale, messages] of Object.entries({
  'zh-CN': zhCN,
  'zh-TW': zhTW,
  en,
  ja,
  ko,
  es,
  fr,
  de,
})) {
  const label = messages.capsule.recordingCountdown;
  assertEqual(typeof label, 'string', `${locale} defines the countdown label`);
  assertEqual(
    [...(label as string).matchAll(/\{\{\w+\}\}/g)].map(([token]) => token).join(','),
    '{{time}}',
    `${locale} keeps the countdown placeholder`,
  );
  assertEqual(
    (label as string).replace('{{time}}', '0:42').includes('0:42'),
    true,
    `${locale} interpolates the time value`,
  );
}

console.log('recording countdown formatting is consistent');
