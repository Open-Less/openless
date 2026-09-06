import type { StyleSystemPrompts, UserPreferences } from "../types"
export type { UpdateChannel } from "../types"
import {
    BACKEND_CONTRACT_VERSION,
    invokeOrMock,
    requireBackendReady,
    type StartupSnapshot,
} from "./shared"
import { mockSettings, mockDefaultStyleSystemPrompts, mockSetSettings } from "./mock-data"

export { BACKEND_CONTRACT_VERSION }
export type { StartupSnapshot }

export async function getStartupSnapshot(): Promise<StartupSnapshot> {
    return requireBackendReady()
}

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
