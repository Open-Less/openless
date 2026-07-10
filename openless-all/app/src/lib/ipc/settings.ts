import type { StyleSystemPrompts, UserPreferences } from "../types"
export type { UpdateChannel } from "../types"
import { invokeOrMock } from "./shared"
import { mockSettings, mockDefaultStyleSystemPrompts, mockSetSettings } from "./mock-data"

export function getSettings(): Promise<UserPreferences> {
    return invokeOrMock("get_settings", undefined, () => ({ ...mockSettings }))
}

export function getDefaultStyleSystemPrompts(): Promise<StyleSystemPrompts> {
    return invokeOrMock("get_default_style_system_prompts", undefined, () => ({
        ...mockDefaultStyleSystemPrompts,
    }))
}

export function setSettings(prefs: UserPreferences): Promise<void> {
    return invokeOrMock("set_settings", { prefs }, () => {
        mockSetSettings(prefs)
        return undefined
    })
}
