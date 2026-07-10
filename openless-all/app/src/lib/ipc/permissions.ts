import type { PermissionStatus } from "../types"
import { invokeOrMock } from "./shared"

export function checkAccessibilityPermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "check_accessibility_permission",
        undefined,
        () => "granted" as const,
    )
}

export function requestAccessibilityPermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "request_accessibility_permission",
        undefined,
        () => "granted" as const,
    )
}

export function checkMicrophonePermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "check_microphone_permission",
        undefined,
        () => "granted" as const,
    )
}

export function requestMicrophonePermission(): Promise<PermissionStatus> {
    return invokeOrMock(
        "request_microphone_permission",
        undefined,
        () => "granted" as const,
    )
}

export function openSystemSettings(
    pane: "accessibility" | "microphone",
): Promise<void> {
    return invokeOrMock("open_system_settings", { pane }, () => undefined)
}

export function triggerMicrophonePrompt(): Promise<void> {
    return invokeOrMock("trigger_microphone_prompt", undefined, () => undefined)
}

export function restartApp(): Promise<void> {
    return invokeOrMock("restart_app", undefined, () => undefined)
}

export function resetAccessibilityPermissionAndRestartApp(): Promise<void> {
    return invokeOrMock(
        "reset_accessibility_permission_and_restart_app",
        undefined,
        () => undefined,
    )
}
