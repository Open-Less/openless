import type { MicrophoneDevice } from "../types"
import { invokeOrMock } from "./shared"
import { mockMicrophoneDevices } from "./mock-data"

export interface NetworkCheckResult {
    online: boolean
    latencyMs: number | null
}

export function checkNetwork(): Promise<NetworkCheckResult> {
    return invokeOrMock<NetworkCheckResult>("check_network", undefined, () => ({
        online: true,
        latencyMs: 42,
    }))
}

export function listMicrophoneDevices(): Promise<MicrophoneDevice[]> {
    return invokeOrMock(
        "list_microphone_devices",
        undefined,
        () => mockMicrophoneDevices,
    )
}

export function startMicrophoneLevelMonitor(deviceName: string): Promise<void> {
    return invokeOrMock(
        "start_microphone_level_monitor",
        { deviceName },
        () => undefined,
    )
}

export function stopMicrophoneLevelMonitor(): Promise<void> {
    return invokeOrMock(
        "stop_microphone_level_monitor",
        undefined,
        () => undefined,
    )
}

export function isWaylandCliMode(): Promise<boolean> {
    return invokeOrMock("is_wayland_cli_mode", undefined, () => false)
}
