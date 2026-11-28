import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(
  new URL('../src/features/capsule/AudioCue.tsx', import.meta.url),
  'utf8',
);

// The always-alive capsule listener must never open output just by mounting, even when
// the persisted preference is disabled. Runtime playback/recovery is covered separately.
assert.doesNotMatch(source, /primeAudioCue/);
assert.equal((source.match(/playRecordStartCue\(\)/g) ?? []).length, 1);
assert.match(source, /if \(audioCueEnabledRef\.current\) playRecordStartCue\(\)/);
assert.match(source, /state === 'recording' && previous !== 'recording'/);
assert.match(source, /host === 'tauri' && os !== 'android'/);
assert.match(source, /await events\.label\(\)\) !== 'capsule'/);
assert.equal(
  (source.match(/if \(!audioCueEnabledRef\.current\) disposeAudioCue\(\)/g) ?? []).length,
  2,
  'both initial settings and live preference changes must dispose disabled audio',
);
assert.match(source, /if \(owned\) disposeAudioCue\(\)/);
console.log('[audioCue.listener.test] lazy startup, preference and ownership contracts passed');
