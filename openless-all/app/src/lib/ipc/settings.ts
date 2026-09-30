import type { StyleSystemPrompts, UserPreferences } from '../types';
export type { UpdateChannel } from '../types';
import {
  BACKEND_CONTRACT_VERSION,
  invokeOrMock,
  requireBackendReady,
  type StartupSnapshot,
} from './shared';
import { mockSettings, mockDefaultStyleSystemPrompts, mockSetSettings } from './mock-data';
import { invalidateMockChannelTests } from './channels';

export { BACKEND_CONTRACT_VERSION };
export type { StartupSnapshot };

export async function getStartupSnapshot(): Promise<StartupSnapshot> {
  return requireBackendReady();
}

export function getSettings(): Promise<UserPreferences> {
  return invokeOrMock('get_settings', undefined, () => ({ ...mockSettings }));
}

export function getDefaultStyleSystemPrompts(): Promise<StyleSystemPrompts> {
  return invokeOrMock('get_default_style_system_prompts', undefined, () => ({
    ...mockDefaultStyleSystemPrompts,
  }));
}

export function setSettings(prefs: UserPreferences): Promise<UserPreferences> {
  return invokeOrMock('set_settings', { prefs }, () => {
    const thinkingChanged = mockSettings.llmThinkingEnabled !== prefs.llmThinkingEnabled;
    mockSetSettings(prefs);
    if (thinkingChanged) invalidateMockChannelTests('llm');
    return prefs;
  });
}

export interface SettingsSnapshot {
  preferences: UserPreferences;
  revision: number;
}
let mockRevision = 0;
export function getSettingsSnapshot(): Promise<SettingsSnapshot> {
  return invokeOrMock('get_settings_snapshot', undefined, () => ({
    preferences: structuredClone(mockSettings),
    revision: mockRevision,
  }));
}
export function updateSettingFields(edits: Record<string, unknown>): Promise<SettingsSnapshot> {
  return invokeOrMock('update_setting_fields', { edits }, () => {
    const prefs = structuredClone(mockSettings);
    for (const [path, value] of Object.entries(edits)) {
      const keys = path
        .slice(1)
        .split('/')
        .map((key) => key.replace(/~1/g, '/').replace(/~0/g, '~'));
      let target = prefs as unknown as Record<string, unknown>;
      for (const key of keys.slice(0, -1)) target = target[key] as Record<string, unknown>;
      target[keys[keys.length - 1]] = value;
    }
    const thinkingChanged = mockSettings.llmThinkingEnabled !== prefs.llmThinkingEnabled;
    mockSetSettings(prefs);
    if (thinkingChanged) invalidateMockChannelTests('llm');
    return { preferences: prefs, revision: ++mockRevision };
  });
}
