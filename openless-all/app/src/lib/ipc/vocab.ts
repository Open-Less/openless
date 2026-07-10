import type { CorrectionRule, DictionaryEntry, VocabPresetStore } from "../types"
import { invokeOrMock } from "./shared"
import { mockVocab, mockCorrectionRules } from "./mock-data"

export function listVocab(): Promise<DictionaryEntry[]> {
    return invokeOrMock("list_vocab", undefined, () => mockVocab)
}

export function addVocab(
    phrase: string,
    note?: string,
): Promise<DictionaryEntry> {
    return invokeOrMock("add_vocab", { phrase, note }, () => ({
        id: `vocab-new-${Date.now()}`,
        phrase,
        note: note ?? null,
        enabled: true,
        hits: 0,
        createdAt: new Date().toISOString(),
    }))
}

export function removeVocab(id: string): Promise<void> {
    return invokeOrMock("remove_vocab", { id }, () => undefined)
}

export function setVocabEnabled(id: string, enabled: boolean): Promise<void> {
    return invokeOrMock("set_vocab_enabled", { id, enabled }, () => undefined)
}

export function listCorrectionRules(): Promise<CorrectionRule[]> {
    return invokeOrMock(
        "list_correction_rules",
        undefined,
        () => mockCorrectionRules,
    )
}

export function addCorrectionRule(
    pattern: string,
    replacement: string,
): Promise<CorrectionRule> {
    return invokeOrMock(
        "add_correction_rule",
        { pattern, replacement },
        () => ({
            id: `rule-new-${Date.now()}`,
            pattern,
            replacement,
            enabled: true,
            createdAt: new Date().toISOString(),
        }),
    )
}

export function removeCorrectionRule(id: string): Promise<void> {
    return invokeOrMock("remove_correction_rule", { id }, () => undefined)
}

export function setCorrectionRuleEnabled(
    id: string,
    enabled: boolean,
): Promise<void> {
    return invokeOrMock(
        "set_correction_rule_enabled",
        { id, enabled },
        () => undefined,
    )
}

export function listVocabPresets(): Promise<VocabPresetStore> {
    return invokeOrMock("list_vocab_presets", undefined, () => ({
        custom: [],
        overrides: [],
        disabledBuiltinPresetIds: [],
    }))
}

export function saveVocabPresets(store: VocabPresetStore): Promise<void> {
    return invokeOrMock("save_vocab_presets", { store }, () => undefined)
}
