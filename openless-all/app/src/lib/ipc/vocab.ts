import type {
    CorrectionRule,
    DictionaryEntry,
    PendingCorrection,
    VocabPresetStore,
} from "../types"
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
            source: "manual" as const,
        }),
    )
}

/** 卡片上点了勾：把这个词收进词汇表，并打「自动收集」标记。 */
export function acceptPendingCorrection(id: string): Promise<void> {
    return invokeOrMock("accept_pending_correction", { id }, () => undefined)
}

/** 卡片上点了叉：丢掉这一条，什么都不记 —— 没有拒绝名单。 */
export function rejectPendingCorrection(id: string): Promise<void> {
    return invokeOrMock("reject_pending_correction", { id }, () => undefined)
}

/** 卡片 10 秒到期，或新一轮听写开始。 */
export function dismissVocabSuggestions(): Promise<void> {
    return invokeOrMock("dismiss_vocab_suggestions", undefined, () => undefined)
}

/**
 * 把文字放进剪贴板。
 *
 * 走后端而不是 `navigator.clipboard`：兜底卡片浮在别的 app 上面、按钮刻意不抢焦点，
 * 而未聚焦的文档调那个 API 会抛 `Document is not focused`。
 */
export function copyTextToClipboard(text: string): Promise<void> {
    return invokeOrMock("copy_text_to_clipboard", { text }, () => undefined)
}

/** 落字失败兜底卡片关掉了（用户点关闭 / TTL 到时）。 */
export function dismissInsertFallbackCard(): Promise<void> {
    return invokeOrMock("dismiss_insert_fallback_card", undefined, () => undefined)
}

/** 把浏览器真实折行后的卡片高度同步给原生共享窗口。 */
export function reportInsertFallbackCardHeight(
    presentationId: number,
    height: number,
): Promise<void> {
    return invokeOrMock(
        "report_insert_fallback_card_height",
        { presentationId, height },
        () => undefined,
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
