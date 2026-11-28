import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const readAndroid = (name) =>
  readFileSync(new URL(`../../android/kotlin/${name}`, import.meta.url), 'utf8');
const source = readAndroid('OpenLessImeService.kt');
const scaffolding = readFileSync(
  new URL('../../android/scripts/copy-android-scaffolding.mjs', import.meta.url),
  'utf8',
);

// Source-only architecture/wiring guards. Touch behavior is exercised by the
// Android View tests, not by matching fragments of a retired key implementation.
test('the host delegates key presentation instead of retaining legacy touch widgets', () => {
  assert.doesNotMatch(source, /internal fun keyboardKey\(/);
  assert.doesNotMatch(source, /class (?:KeyPreviewOverlay|SwipeModeContainer|StrokeKeyView)\b/);
  assert.match(source, /ImeTypingPanel\(/);
  assert.match(source, /ImeClipboardPanel\(/);
  assert.match(readAndroid('StrokeInputController.kt'), /ImeStrokePanel\(/);
  for (const name of [
    'ImeActionButton.kt',
    'ImeCandidateStrip.kt',
    'ImeTypingPanel.kt',
    'ImeVoicePanel.kt',
    'ImeClipboardPanel.kt',
    'ImeStrokePanel.kt',
    'ImeDraftBar.kt',
    'ImeSurfaceHost.kt',
    'ImeKeyboardPreview.kt',
  ]) {
    const view = readAndroid(name);
    assert.doesNotMatch(
      view,
      /OpenLessNative\.|currentInputConnection|getSharedPreferences\(/,
      `${name} must not own backend calls, editor connections, or preferences`,
    );
    assert.ok(scaffolding.includes(`'${name}'`), `${name} must reach the Android build`);
  }
});

test('normal presentation refreshes retain the system input-view root', () => {
  assert.match(source, /new ImeSurfaceHost|ImeSurfaceHost\(this\)/);
  assert.match(source, /surfaceHost\?\.render\(keyboard, draft, draftEdit\)/);
  assert.equal((source.match(/setInputView\(onCreateInputView\(\)\)/g) ?? []).length, 1);
  const refresh = source.slice(
    source.indexOf('internal fun refreshInputView()'),
    source.indexOf('private fun refreshInputView(slideDirection:'),
  );
  assert.match(refresh, /replaceSurfaceContent\(\)/);
  assert.doesNotMatch(refresh, /setInputView\(/);
});

test('key and candidate behavior fixtures reach the Android instrumentation source set', () => {
  for (const name of [
    'ImeTypingPanelInstrumentedTest.kt',
    'ImeVoicePanelInstrumentedTest.kt',
    'ImeRemainingPanelsInstrumentedTest.kt',
  ]) {
    assert.ok(scaffolding.includes(`'${name}'`), `${name} must be staged`);
    assert.match(readAndroid(`androidTest/${name}`), /@RunWith\(AndroidJUnit4::class\)/);
  }
});

test('a reused English letter panel synchronizes Shift even when its state does not change', () => {
  const startView = source.slice(
    source.indexOf('override fun onStartInputView('),
    source.indexOf('private fun keyboardFootprintKey('),
  );
  assert.match(startView, /updateAutoShift\(refresh = false\)/);
  assert.match(
    startView,
    /else if \(inputMode == InputMode\.ENGLISH && englishLayer == EnglishLayer\.LETTERS\) \{\s*applyShiftState\(\)/,
    'onStartInput resets automatic Shift before a cached panel is shown again',
  );
});
