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

export function setCredential(account: string, value: string): Promise<void> {
    return invokeOrMock("set_credential", { account, value }, () => undefined)
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

export function readCredential(account: string): Promise<string | null> {
    return invokeOrMock<string | null>(
        "read_credential",
        { account },
        () => null,
    )
}

export function validateProviderCredentials(
    kind: "llm" | "asr",
): Promise<ProviderCheckResult> {
    return invokeOrMock("validate_provider_credentials", { kind }, () => ({
        ok: true,
    }))
}

export function listProviderModels(
    kind: "llm" | "asr",
): Promise<ProviderModelsResult> {
    return invokeOrMock("list_provider_models", { kind }, () => ({
        models:
            kind === "llm"
                ? ["gpt-4o", "deepseek-v4-flash", "deepseek-v4-pro"]
                : ["whisper-1"],
    }))
}
