import type { HotkeyCapability, HotkeyStatus, PlatformCapabilities, WindowsImeStatus } from "../types"
import { getPlatformCapabilities as loadPlatformCapabilities } from "../platform"

declare global {
    interface Window {
        __TAURI_INTERNALS__?: unknown
    }
}

export const isTauri =
    globalThis.window !== undefined &&
    "__TAURI_INTERNALS__" in globalThis.window

let platformCapsPromise: Promise<PlatformCapabilities> | null = null

export async function platformCapabilities(): Promise<PlatformCapabilities> {
    platformCapsPromise ??= loadPlatformCapabilities()
    return platformCapsPromise
}

export async function getPlatformCapabilities(): Promise<PlatformCapabilities> {
    return platformCapabilities()
}

export async function invokeOrMock<T>(
    cmd: string,
    args: Record<string, unknown> | undefined,
    mock: () => T,
): Promise<T> {
    if (!isTauri) {
        return mock()
    }
    const { invoke } = await import("@tauri-apps/api/core")
    return invoke<T>(cmd, args)
}

export const androidHotkeyCapability: HotkeyCapability = {
    adapter: "unavailable",
    availableTriggers: [],
    requiresAccessibilityPermission: false,
    supportsModifierOnlyTrigger: false,
    supportsSideSpecificModifiers: false,
    explicitFallbackAvailable: false,
    statusHint:
        "移动端不支持全局热键；请使用应用内录音按钮或悬浮窗（需授权）。",
}

export const androidHotkeyStatus: HotkeyStatus = {
    adapter: "unavailable",
    state: "failed",
    message: "移动端不支持全局热键",
    lastError: {
        code: "unavailable",
        message: "Global hotkeys are not available on mobile",
    },
}

export const androidWindowsImeStatus: WindowsImeStatus = {
    state: "notWindows",
    usingTsfBackend: false,
    message: "Not available on Android",
    dllPath: null,
}
