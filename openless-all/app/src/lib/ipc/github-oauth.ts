import { invokeOrMock } from "./shared"

export interface GithubDeviceStartResponse {
    flowId: string
    userCode: string
    verificationUri: string
    interval: number
    expiresIn: number
}

export type GithubDevicePollResult =
    | { kind: "authorized"; login: string }
    | { kind: "pending" }
    | { kind: "slowDown" }
    | { kind: "error"; message: string }

export interface MarketplaceAuthStatus {
    signedIn: boolean
}

export function githubDeviceFlowStart(): Promise<GithubDeviceStartResponse> {
    return invokeOrMock<GithubDeviceStartResponse>(
        "github_device_flow_start",
        undefined,
        () => ({
            flowId: "mock-opaque-flow-id",
            userCode: "MOCK-CODE",
            verificationUri: "https://github.com/login/device",
            interval: 5,
            expiresIn: 900,
        }),
    )
}

export function githubDeviceFlowPoll(
    flowId: string,
): Promise<GithubDevicePollResult> {
    return invokeOrMock<GithubDevicePollResult>(
        "github_device_flow_poll",
        { flowId },
        () => ({
            kind: "authorized" as const,
            login: "mock-user",
        }),
    )
}

export function githubDeviceFlowCancel(flowId?: string): Promise<void> {
    return invokeOrMock<void>(
        "github_device_flow_cancel",
        { flowId: flowId ?? null },
        () => undefined,
    )
}

export function githubPollIntervalMs(intervalSeconds: number): number {
    if (!Number.isFinite(intervalSeconds) || intervalSeconds <= 0) return 5_000
    return Math.ceil(intervalSeconds * 1_000)
}

export function githubSlowDownIntervalMs(currentIntervalMs: number): number {
    return Math.max(0, currentIntervalMs) + 5_000
}

export function githubFlowExpiresAt(nowMs: number, expiresInSeconds: number): number {
    return nowMs + Math.max(0, expiresInSeconds) * 1_000
}

export function marketplaceAuthStatus(): Promise<MarketplaceAuthStatus> {
    return invokeOrMock<MarketplaceAuthStatus>(
        "marketplace_auth_status",
        undefined,
        () => ({ signedIn: false }),
    )
}

export function marketplaceLogout(): Promise<void> {
    return invokeOrMock<void>("marketplace_logout", undefined, () => undefined)
}
