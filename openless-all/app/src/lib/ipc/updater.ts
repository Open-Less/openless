import type { UpdateChannel } from "../types"
import { invokeOrMock, platformCapabilities } from "./shared"

export interface LatestBetaRelease {
    tagName: string
    htmlUrl: string
    publishedAt: string
}

export interface AppUpdateMetadata {
    rid: number
    currentVersion: string
    version: string
    date?: string | null
    body?: string | null
    rawJson: Record<string, unknown>
}

export function getUpdateChannel(): Promise<UpdateChannel> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return "stable" as UpdateChannel
        }
        return invokeOrMock(
            "get_update_channel",
            undefined,
            () => "stable" as UpdateChannel,
        )
    })
}

export function setUpdateChannel(channel: UpdateChannel): Promise<void> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return undefined
        }
        return invokeOrMock("set_update_channel", { channel }, () => undefined)
    })
}

export function fetchLatestBetaRelease(): Promise<LatestBetaRelease | null> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return null
        }
        return invokeOrMock("fetch_latest_beta_release", undefined, () => null)
    })
}

export function appCheckUpdateWithChannel(
    timeoutMs: number,
    channel?: UpdateChannel | null,
): Promise<AppUpdateMetadata | null> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsAutoUpdate) {
            return null
        }
        return invokeOrMock(
            "app_check_update_with_channel",
            { timeoutMs, channel: channel ?? null },
            () => null,
        )
    })
}

export function appDownloadAndInstallAndroidUpdate(args: {
    url: string
    signature: string
    version: string
}): Promise<void> {
    return platformCapabilities().then((caps) => {
        if (caps.platform !== "android") {
            return Promise.reject(new Error("Android-only update install"))
        }
        return invokeOrMock(
            "app_download_and_install_android_update",
            args,
            () => undefined,
        )
    })
}
