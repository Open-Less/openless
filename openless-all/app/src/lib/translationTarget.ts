// Availability rules for the translation target language, kept identical to the backend's `types.rs::translation_effective`.
// The backend uses it when the translation modifier is pressed to decide whether to enter the translation
// pipeline; this module only warns the translation page in advance about "set but won't take effect"
// combinations, avoiding the silent failure of "pressed Shift and nothing translated".

/** No target language selected = translation disabled (Shift does nothing). */
export function isTranslationEnabled(translationTargetLanguage: string): boolean {
  return translationTargetLanguage.trim() !== '';
}

/**
 * The target language equals the user's single working language — the source language is necessarily the
 * target, so translation is a provable no-op.
 *
 * Returns false when there are multiple working languages: a bilingual user setting English as the target
 * while speaking Chinese is normal usage, and the source language can't be determined in advance, so it
 * must not be blocked. Simplified/Traditional are separate entries in the language list and compare
 * literally, so Simplified→Traditional is not misjudged as a no-op.
 */
export function isTranslationTargetRedundant(
  translationTargetLanguage: string,
  workingLanguages: readonly string[],
): boolean {
  const target = translationTargetLanguage.trim();
  if (target === '') return false;
  return workingLanguages.length === 1 && workingLanguages[0].trim() === target;
}
