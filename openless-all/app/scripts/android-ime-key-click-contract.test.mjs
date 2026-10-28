import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const source = readFileSync(
  new URL('../android/kotlin/OpenLessImeService.kt', import.meta.url),
  'utf8',
);
const keyboardKey = source.slice(
  source.indexOf('internal fun keyboardKey('),
  source.indexOf('private fun keyRepeatIntervalMs('),
);

test('a fresh key press clears click suppression left by an out-of-bounds swipe', () => {
  const down = keyboardKey.match(/MotionEvent\.ACTION_DOWN -> \{([\s\S]*?)MotionEvent\.ACTION_MOVE/);
  assert.ok(down, 'keyboardKey must handle ACTION_DOWN');
  assert.match(down[1], /suppressNextClick = false/,
    'an out-of-bounds swipe has no native click to clear suppression before the next tap');
});

test('a swipe still suppresses the click from the same touch sequence', () => {
  assert.match(keyboardKey,
    /if \(event\.actionMasked == MotionEvent\.ACTION_UP && swipePreviewShown\) \{\s*swipeSpecOf\(trackedView\)\?\.first\?\.invoke\(\)\s*suppressNextClick = true/);
  assert.match(keyboardKey,
    /setOnClickListener \{\s*if \(suppressNextClick\) \{\s*suppressNextClick = false\s*\} else \{\s*action\(\)/);
});

test('a reused English letter panel synchronizes Shift even when its state does not change', () => {
  const startView = source.slice(
    source.indexOf('override fun onStartInputView('),
    source.indexOf('private fun keyboardFootprintKey('),
  );
  assert.match(startView, /updateAutoShift\(refresh = false\)/);
  assert.match(startView,
    /else if \(inputMode == InputMode\.ENGLISH && englishLayer == EnglishLayer\.LETTERS\) \{\s*applyShiftState\(\)/,
    'onStartInput resets automatic Shift before a cached panel is shown again');
});
