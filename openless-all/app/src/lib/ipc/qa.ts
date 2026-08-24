import type { QaHotkeyBinding } from "../types"
import { invokeOrMock } from "./shared"
import { formatComboLabel, defaultQaShortcut } from "../hotkey"

export function getQaHotkeyLabel(): Promise<string> {
    return invokeOrMock("get_qa_hotkey_label", undefined, () =>
        formatComboLabel(defaultQaShortcut()),
    )
}

export function setQaHotkey(binding: QaHotkeyBinding | null): Promise<void> {
    return invokeOrMock("set_qa_hotkey", { binding }, () => undefined)
}

export function qaWindowDismiss(): Promise<void> {
    return invokeOrMock("qa_window_dismiss", undefined, () => undefined)
}

export function qaToggleRecording(): Promise<void> {
    return invokeOrMock("qa_toggle_recording", undefined, () => undefined)
}

export function qaSubmitText(text: string): Promise<void> {
    return invokeOrMock("qa_submit_text", { text }, () => undefined)
}

export function qaSetEditInstructionMode(enabled: boolean): Promise<void> {
    return invokeOrMock("qa_set_edit_instruction_mode", { enabled }, () => undefined)
}
