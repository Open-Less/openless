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

/** 沿用原 session id 与 WAV，继续一条被 Esc 打断的录音。 */
export function resumeCancelledRecording(sessionId: string): Promise<void> {
    return invokeOrMock(
        "resume_cancelled_recording",
        { sessionId },
        () => undefined,
    )
}

/** 主动收起 3 秒恢复提示；历史记录与 WAV 不受影响。 */
export function dismissCancelledRecordingRecovery(sessionId?: string): Promise<void> {
    return invokeOrMock(
        "dismiss_cancelled_recording_recovery",
        { sessionId: sessionId ?? null },
        () => undefined,
    )
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
