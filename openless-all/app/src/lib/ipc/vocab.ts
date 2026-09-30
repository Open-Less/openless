import type { CorrectionRule, DictionaryEntry, VocabPresetStore } from '../types';
import { invokeOrMock } from './shared';
import { mockVocab, mockCorrectionRules } from './mock-data';

export function listVocab(): Promise<DictionaryEntry[]> {
  return invokeOrMock('list_vocab', undefined, () => mockVocab.map((entry) => ({ ...entry })));
}

export function addLearnedVocab(phrase: string): Promise<void> {
  return invokeOrMock('add_learned_vocab', { phrase }, () => {
    if (!mockVocab.some((entry) => entry.phrase === phrase.trim())) {
      mockVocab.unshift({
        id: crypto.randomUUID(),
        phrase: phrase.trim(),
        note: '从手改中自动收集',
        enabled: true,
        hits: 0,
        createdAt: new Date().toISOString(),
      });
    }
  });
}

export function addVocab(phrase: string, note?: string): Promise<DictionaryEntry> {
  return invokeOrMock('add_vocab', { phrase, note }, () => {
    const entry = {
      id: crypto.randomUUID(),
      phrase,
      note: note ?? null,
      enabled: true,
      hits: 0,
      createdAt: new Date().toISOString(),
    };
    mockVocab.unshift(entry);
    return { ...entry };
  });
}

export function removeVocab(id: string): Promise<void> {
  return invokeOrMock('remove_vocab', { id }, () => {
    const index = mockVocab.findIndex((entry) => entry.id === id);
    if (index >= 0) mockVocab.splice(index, 1);
  });
}

export function setVocabEnabled(id: string, enabled: boolean): Promise<void> {
  return invokeOrMock('set_vocab_enabled', { id, enabled }, () => {
    const entry = mockVocab.find((entry) => entry.id === id);
    if (entry) entry.enabled = enabled;
  });
}

/** Edit an entry's text: id / hits / enabled stay unchanged (backend update_vocab). */
export function updateVocab(id: string, phrase: string): Promise<void> {
  return invokeOrMock('update_vocab', { id, phrase }, () => {
    const entry = mockVocab.find((entry) => entry.id === id);
    if (entry) entry.phrase = phrase;
  });
}

export function listCorrectionRules(): Promise<CorrectionRule[]> {
  return invokeOrMock('list_correction_rules', undefined, () => mockCorrectionRules);
}

export function addCorrectionRule(pattern: string, replacement: string): Promise<CorrectionRule> {
  return invokeOrMock('add_correction_rule', { pattern, replacement }, () => ({
    id: `rule-new-${Date.now()}`,
    pattern,
    replacement,
    enabled: true,
    createdAt: new Date().toISOString(),
    source: 'manual' as const,
  }));
}

/** Check mark clicked on the card: add this word to the vocab list with the "auto-collected"
 *  flag. */
export function acceptPendingCorrection(id: string): Promise<void> {
  return invokeOrMock('accept_pending_correction', { id }, () => undefined);
}

/** Cross clicked on the card: drop this item, recording nothing — there is no denylist. */
export function rejectPendingCorrection(id: string): Promise<void> {
  return invokeOrMock('reject_pending_correction', { id }, () => undefined);
}

/** Dismiss on the configured deadline or the next dictation round. */
export function dismissVocabSuggestions(): Promise<void> {
  return invokeOrMock('dismiss_vocab_suggestions', undefined, () => undefined);
}

/**
 * Put text into the clipboard.
 *
 * Goes through the backend rather than `navigator.clipboard`: the fallback card floats over
 * another app and the buttons deliberately do not steal focus, while an unfocused document calling
 * that API throws `Document is not focused`.
 */
export function copyTextToClipboard(text: string): Promise<void> {
  return invokeOrMock('copy_text_to_clipboard', { text }, () => undefined);
}

/** The insert-fallback card closed (user clicked close / TTL expired). */
export function dismissInsertFallbackCard(): Promise<void> {
  return invokeOrMock('dismiss_insert_fallback_card', undefined, () => undefined);
}

/** Sync the card's real browser-wrapped height to the native shared window. */
export function reportInsertFallbackCardHeight(
  presentationId: number,
  height: number,
): Promise<void> {
  return invokeOrMock(
    'report_insert_fallback_card_height',
    { presentationId, height },
    () => undefined,
  );
}

export function removeCorrectionRule(id: string): Promise<void> {
  return invokeOrMock('remove_correction_rule', { id }, () => undefined);
}

export function setCorrectionRuleEnabled(id: string, enabled: boolean): Promise<void> {
  return invokeOrMock('set_correction_rule_enabled', { id, enabled }, () => undefined);
}

export function listVocabPresets(): Promise<VocabPresetStore> {
  return invokeOrMock('list_vocab_presets', undefined, () => ({
    custom: [],
    overrides: [],
    disabledBuiltinPresetIds: [],
  }));
}

export function saveVocabPresets(store: VocabPresetStore): Promise<void> {
  return invokeOrMock('save_vocab_presets', { store }, () => undefined);
}
