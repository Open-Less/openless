import type { CredentialsStatus } from "../types"
import { invokeOrMock } from "./shared"
import { mockCredentialsStatus } from "./mock-data"

export interface ProviderCheckResult {
    ok: boolean
}

export interface ProviderModelsResult {
    models: string[]
}

export function getCredentials(): Promise<CredentialsStatus> {
    return invokeOrMock(
        "get_credentials",
        undefined,
        () => mockCredentialsStatus,
    )
}

export function setCredential(account: string, value: string, provider?: string): Promise<void> {
    return invokeOrMock("set_credential", { account, value, provider }, () => undefined)
}

export function setActiveAsrProvider(provider: string): Promise<void> {
    return invokeOrMock(
        "set_active_asr_provider",
        { provider },
        () => undefined,
    )
}

export function setActiveLlmProvider(provider: string): Promise<void> {
    return invokeOrMock(
        "set_active_llm_provider",
        { provider },
        () => undefined,
    )
}

export function setActiveOmniProvider(provider: string): Promise<void> {
    return invokeOrMock(
        "set_active_omni_provider",
        { provider },
        () => undefined,
    )
}

export function readCredential(account: string, provider?: string): Promise<string | null> {
    return invokeOrMock<string | null>(
        "read_credential",
        { account, provider },
        () => null,
    )
}

export function validateProviderCredentials(
    kind: "llm" | "asr" | "omni",
): Promise<ProviderCheckResult> {
    return invokeOrMock("validate_provider_credentials", { kind }, () => ({
        ok: true,
    }))
}

export function listProviderModels(
    kind: "llm" | "asr" | "omni",
): Promise<ProviderModelsResult> {
    return invokeOrMock("list_provider_models", { kind }, () => ({
        models:
            kind === "llm"
                ? ["gpt-4o", "deepseek-v4-flash", "deepseek-v4-pro"]
                : ["whisper-1"],
    }))
}
