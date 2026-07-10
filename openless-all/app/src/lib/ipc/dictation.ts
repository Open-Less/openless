import { invokeOrMock, platformCapabilities } from "./shared"

export function startDictation(): Promise<void> {
    return invokeOrMock("start_dictation", undefined, () => undefined)
}

export function stopDictation(): Promise<void> {
    return invokeOrMock("stop_dictation", undefined, () => undefined)
}

export function cancelDictation(): Promise<void> {
    return invokeOrMock("cancel_dictation", undefined, () => undefined)
}

export function handleWindowHotkeyEvent(
    eventType: "keydown" | "keyup",
    key: string,
    code: string,
    repeat: boolean,
): Promise<void> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsDesktopHotkey) {
            return undefined
        }
        return invokeOrMock(
            "handle_window_hotkey_event",
            { event_type: eventType, key, code, repeat },
            () => undefined,
        )
    })
}
