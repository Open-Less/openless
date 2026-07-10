import type { ComboBinding, HotkeyCapability, HotkeyStatus, ShortcutBinding, WindowsImeStatus } from "../types"
import { invokeOrMock, platformCapabilities, androidHotkeyStatus, androidHotkeyCapability, androidWindowsImeStatus } from "./shared"
import { mockHotkeyStatus, mockHotkeyCapability, mockWindowsImeStatus } from "./mock-data"

export function getHotkeyStatus(): Promise<HotkeyStatus> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsDesktopHotkey) {
            return androidHotkeyStatus
        }
        return invokeOrMock("get_hotkey_status", undefined, () => mockHotkeyStatus)
    })
}

export function getHotkeyCapability(): Promise<HotkeyCapability> {
    return platformCapabilities().then((caps) => {
        if (!caps.supportsDesktopHotkey) {
            return androidHotkeyCapability
        }
        return invokeOrMock(
            "get_hotkey_capability",
            undefined,
            () => mockHotkeyCapability,
        )
    })
}

export function getWindowsImeStatus(): Promise<WindowsImeStatus> {
    return platformCapabilities().then((caps) => {
        if (caps.platform === "android") {
            return androidWindowsImeStatus
        }
        return invokeOrMock(
            "get_windows_ime_status",
            undefined,
            () => mockWindowsImeStatus,
        )
    })
}

export function validateComboHotkey(binding: ComboBinding): Promise<void> {
    return invokeOrMock("validate_combo_hotkey", { binding }, () => undefined)
}

export function setComboHotkey(binding: ComboBinding): Promise<void> {
    return invokeOrMock("set_combo_hotkey", { binding }, () => undefined)
}

export function validateShortcutBinding(
    binding: ShortcutBinding,
): Promise<void> {
    return invokeOrMock(
        "validate_shortcut_binding",
        { binding },
        () => undefined,
    )
}

export function setDictationHotkey(binding: ShortcutBinding): Promise<void> {
    return invokeOrMock("set_dictation_hotkey", { binding }, () => undefined)
}

export function setTranslationHotkey(binding: ShortcutBinding): Promise<void> {
    return invokeOrMock("set_translation_hotkey", { binding }, () => undefined)
}

// binding = null 表示停用（清空全局键），与 set_qa_hotkey 一致（issue #576）。
export function setSwitchStyleHotkey(binding: ShortcutBinding | null): Promise<void> {
    return invokeOrMock("set_switch_style_hotkey", { binding }, () => undefined)
}

export function setOpenAppHotkey(binding: ShortcutBinding | null): Promise<void> {
    return invokeOrMock("set_open_app_hotkey", { binding }, () => undefined)
}

export function setShortcutRecordingActive(active: boolean): Promise<void> {
    return invokeOrMock(
        "set_shortcut_recording_active",
        { active },
        () => undefined,
    )
}
