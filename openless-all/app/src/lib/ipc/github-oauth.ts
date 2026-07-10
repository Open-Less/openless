import { invokeOrMock } from "./shared"

export interface GithubDeviceStartResponse {
    deviceCode: string
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

export function githubDeviceFlowStart(): Promise<GithubDeviceStartResponse> {
    return invokeOrMock<GithubDeviceStartResponse>(
        "github_device_flow_start",
        undefined,
        () => ({
            deviceCode: "mock-device-code-xxxxxxxx",
            userCode: "MOCK-CODE",
            verificationUri: "https://github.com/login/device",
            interval: 5,
            expiresIn: 900,
        }),
    )
}

export function githubDeviceFlowPoll(
    deviceCode: string,
): Promise<GithubDevicePollResult> {
    return invokeOrMock<GithubDevicePollResult>(
        "github_device_flow_poll",
        { deviceCode },
        () => ({
            kind: "authorized" as const,
            login: "mock-user",
        }),
    )
}
