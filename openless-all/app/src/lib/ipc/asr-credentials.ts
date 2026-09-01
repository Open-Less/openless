import type { CredentialsStatus } from "../types"
import { invokeOrMock, isTauri } from "./shared"
import { mockCredentialsStatus } from "./mock-data"

const browserCredentialMock = new Map<string, string>([
    ["orcarouter-asr:asr.endpoint", "https://api.orcarouter.ai/v1"],
    ["orcarouter-asr:asr.model", "google/gemini-2.5-flash"],
])

function browserCredentialKey(account: string, provider?: string): string {
    return `${provider ?? "active"}:${account}`
}

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
    return invokeOrMock("set_credential", { account, value, provider }, () => {
        browserCredentialMock.set(browserCredentialKey(account, provider), value)
    })
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
        () => browserCredentialMock.get(browserCredentialKey(account, provider)) ?? null,
    )
}

/** `channelId` 省略时测当前生效的渠道；卡片上的「测试连通」会带上那张卡片的 id。 */
export function validateProviderCredentials(
    kind: "llm" | "asr" | "omni",
    channelId?: string,
): Promise<ProviderCheckResult> {
    return invokeOrMock("validate_provider_credentials", { kind, channelId }, () => ({
        ok: true,
    }))
}

export async function listProviderModels(
    kind: "llm" | "asr" | "omni",
    channelId?: string,
): Promise<ProviderModelsResult> {
    if (!isTauri && (kind === "llm" || kind === "asr")) {
        const endpointAccount = kind === "llm" ? "ark.endpoint" : "asr.endpoint"
        const endpoint = browserCredentialMock.get(
            browserCredentialKey(endpointAccount, channelId),
        )
        if (endpoint) {
            const host = new URL(endpoint).hostname.toLowerCase()
            if (host === "api.orcarouter.ai") {
                const response = await fetch("/__openless_dev/orcarouter/models")
                if (!response.ok) {
                    throw new Error(`OrcaRouter /models returned ${response.status}`)
                }
                const payload = await response.json() as {
                    data?: Array<{
                        id?: string
                        supported_endpoint_types?: string[]
                    }>
                }
                return {
                    models: (payload.data ?? [])
                        .filter(model => (
                            !model.supported_endpoint_types
                            || model.supported_endpoint_types.includes("openai")
                        ))
                        .map(model => model.id?.trim() ?? "")
                        .filter(model => {
                            if (!model) return false
                            if (kind === "llm") return true
                            const id = model.toLowerCase()
                            return id.startsWith("google/gemini")
                                && !/(image|tts|embedding|robotics)/.test(id)
                        }),
                }
            }
        }
    }
    return invokeOrMock("list_provider_models", { kind, channelId }, () => ({
        models:
            kind === "llm"
                ? ["gpt-4o", "deepseek-v4-flash", "deepseek-v4-pro"]
                : ["whisper-1"],
    }))
}
