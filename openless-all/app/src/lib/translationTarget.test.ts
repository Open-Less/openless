import { isTranslationEnabled, isTranslationTargetRedundant } from './translationTarget';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

// No target selected = feature disabled.
assert(isTranslationEnabled('') === false, 'empty target should read as disabled');
assert(isTranslationEnabled('   ') === false, 'blank target should read as disabled');
assert(isTranslationEnabled('English') === true, 'a chosen target should read as enabled');

// Target = the only working language: translation is a no-op, the page must warn.
assert(
  isTranslationTargetRedundant('简体中文', ['简体中文']) === true,
  'target equal to the only working language should be flagged redundant',
);
assert(
  isTranslationTargetRedundant(' 简体中文 ', ['简体中文']) === true,
  'surrounding whitespace should not hide a redundant target',
);

// Simplified→Traditional is a real conversion; must not be misjudged as a no-op.
assert(
  isTranslationTargetRedundant('繁体中文', ['简体中文']) === false,
  'simplified to traditional is a real conversion',
);

// Multiple working languages are never blocked: speaking Chinese with English output is normal usage.
assert(
  isTranslationTargetRedundant('English', ['简体中文', 'English']) === false,
  'multiple working languages should never be flagged',
);

// With no target selected, the "disabled" path applies; it must not also report "redundant".
assert(
  isTranslationTargetRedundant('', ['简体中文']) === false,
  'an unset target is disabled, not redundant',
);
assert(
  isTranslationTargetRedundant('English', []) === false,
  'no working languages means nothing to compare against',
);
