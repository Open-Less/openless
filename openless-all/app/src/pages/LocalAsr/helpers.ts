// Pure UI helpers extracted from LocalAsr/index.tsx (behavior-preserving move).
// Alias/language-hint guards, platform detection, and size formatting used by the
// Local ASR page and its sub-components.

import {
    FOUNDRY_LOCAL_ASR_MODELS,
    SHERPA_ONNX_ASR_MODELS,
    type FoundryLocalAsrLanguageHint,
    type FoundryLocalAsrModelAlias,
    type FoundryRuntimeSource,
    type SherpaOnnxLanguageHint,
    type SherpaOnnxModelAlias,
} from "../../lib/localAsr"

export function isFoundryAlias(value: string): value is FoundryLocalAsrModelAlias {
    return FOUNDRY_LOCAL_ASR_MODELS.some((model) => model.alias === value)
}

export function isSherpaAlias(value: string): value is SherpaOnnxModelAlias {
    return SHERPA_ONNX_ASR_MODELS.some((model) => model.alias === value)
}

export function normalizeFoundryLanguageHintForUi(
    value: string,
): FoundryLocalAsrLanguageHint {
    return value === "zh" || value === "en" ? value : ""
}

export function normalizeSherpaLanguageHintForUi(
    value: string,
): SherpaOnnxLanguageHint {
    return value === "zh" ||
        value === "en" ||
        value === "ja" ||
        value === "ko" ||
        value === "yue"
        ? value
        : ""
}

export function normalizeFoundryRuntimeSourceForUi(
    value: string,
): FoundryRuntimeSource {
    return value === "nuget" || value === "ort-nightly" ? value : "auto"
}

export function isWindowsLikePlatform(): boolean {
    const nav = navigator as Navigator & {
        userAgentData?: { platform?: string }
    }
    const platform =
        nav.userAgentData?.platform || navigator.platform || navigator.userAgent
    return /win/i.test(platform)
}

export function formatFoundrySizeMb(
    fileSizeMb: number | null | undefined,
): string | null {
    if (typeof fileSizeMb !== "number" || fileSizeMb <= 0) return null
    return Math.round(fileSizeMb).toLocaleString()
}

export function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(0)} MB`
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
}
