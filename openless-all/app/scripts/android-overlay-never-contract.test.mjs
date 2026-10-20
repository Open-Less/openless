import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = (name) =>
  readFileSync(new URL(`../android/kotlin/${name}.kt`, import.meta.url), 'utf8');
const accessibility = source('OpenLessAccessibilityService');
const overlay = source('OpenLessOverlayService');

assert.match(
  accessibility,
  /private fun refreshVocabularyCard\(\)\s*\{\s*val request = \+\+vocabularyCardRequest\s*if \(!OpenLessAndroidPreferences\.overlayDisplayAllowed\(this\)\)\s*\{\s*hideVocabularyCard\(\)\s*return\s*\}\s*requestVocabulary\("pending"\)/,
  'Disabling overlays must invalidate old replies and hide the card without waiting for pending-suggestion IPC',
);
assert.match(
  accessibility,
  /private fun showVocabularyCard\(json: String\)\s*\{\s*hideVocabularyCard\(\)\s*if \(!OpenLessAndroidPreferences\.overlayDisplayAllowed\(this\)\) return/,
  'Every vocabulary card render, including delayed IPC replies, must hide the old card and honor never before creating a new one',
);
assert.match(
  overlay,
  /private fun hideOverlay\(\)[\s\S]*?clearAllOverlayRoots\(\)[\s\S]*?if \(!overlayDisplayAllowed\(\)\)\s*\{\s*OpenLessAccessibilityService\.sendVocabularyCommand\("refresh"\)/,
  'Hiding overlays in never mode must also refresh cards in the accessibility process',
);
assert.match(
  accessibility,
  /internal fun sendVocabularyCommand\([\s\S]*?ACTION_VOCABULARY[\s\S]*?context\.sendBroadcast\(intent\)/,
  'The overlay service must reuse the cross-process vocabulary command',
);
assert.match(
  accessibility,
  /"refresh" -> service\.refreshVocabularyCard\(\)/,
  'The accessibility process must refresh the card after a preference change',
);

console.log('android overlay never contract tests passed');
