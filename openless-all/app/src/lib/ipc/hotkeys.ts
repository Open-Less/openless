import type {
  ComboBinding,
  HotkeyCapability,
  HotkeyStatus,
  ShortcutBinding,
  StylePackHotkey,
  WindowsImeStatus,
} from '../types';
import {
  invokeOrMock,
  platformCapabilities,
  androidHotkeyStatus,
  androidHotkeyCapability,
  androidWindowsImeStatus,
} from './shared';
import {
  mockHotkeyStatus,
  mockHotkeyCapability,
  mockSettings,
  mockSetSettings,
  mockWindowsImeStatus,
} from './mock-data';

export function getHotkeyStatus(): Promise<HotkeyStatus> {
  return platformCapabilities().then((caps) => {
    if (!caps.supportsDesktopHotkey) {
      return androidHotkeyStatus;
    }
    return invokeOrMock('get_hotkey_status', undefined, () => mockHotkeyStatus);
  });
}

export function getHotkeyCapability(): Promise<HotkeyCapability> {
  return platformCapabilities().then((caps) => {
    if (!caps.supportsDesktopHotkey) {
      return androidHotkeyCapability;
    }
    return invokeOrMock('get_hotkey_capability', undefined, () => mockHotkeyCapability);
  });
}

/** Linux 输入法后端（fcitx5 插件）状态：插件是否安装、守护进程是否运行。 */
export interface ImeBackendStatus {
  backend: 'fcitx5';
  pluginInstalled: boolean;
  daemonRunning: boolean;
  ready: boolean;
}

export function getImeBackendStatus(): Promise<ImeBackendStatus> {
  return platformCapabilities().then((caps) =>
    caps.linuxHost
      ? invokeOrMock<ImeBackendStatus>('get_ime_backend_status', undefined, () => ({
          backend: 'fcitx5',
          pluginInstalled: true,
          daemonRunning: true,
          ready: true,
        }))
      : { backend: 'fcitx5', pluginInstalled: false, daemonRunning: false, ready: false },
  );
}

export function getWindowsImeStatus(): Promise<WindowsImeStatus> {
  return platformCapabilities().then((caps) => {
    if (caps.platform !== 'desktop') {
      return androidWindowsImeStatus;
    }
    return invokeOrMock('get_windows_ime_status', undefined, () => mockWindowsImeStatus);
  });
}

export function validateComboHotkey(binding: ComboBinding): Promise<void> {
  return invokeOrMock('validate_combo_hotkey', { binding }, () => undefined);
}

export function setComboHotkey(binding: ComboBinding): Promise<void> {
  return invokeOrMock('set_combo_hotkey', { binding }, () => undefined);
}

export function validateShortcutBinding(binding: ShortcutBinding): Promise<void> {
  return invokeOrMock('validate_shortcut_binding', { binding }, () => undefined);
}

export function setDictationHotkey(binding: ShortcutBinding | null): Promise<void> {
  return invokeOrMock('set_dictation_hotkey', { binding }, () => {
    mockSetSettings({
      ...mockSettings,
      dictationHotkey: binding ?? { primary: '', modifiers: [] },
    });
  });
}

// binding = null disables the hotkey. Once the Tauri side persists, the settings page re-reads prefs immediately.
export function setSelectionPolishHotkey(binding: ShortcutBinding | null): Promise<void> {
  return invokeOrMock('set_selection_polish_hotkey', { binding }, () => {
    mockSetSettings({ ...mockSettings, selectionPolishHotkey: binding });
    return undefined;
  });
}

export function setTranslationHotkey(binding: ShortcutBinding | null): Promise<void> {
  return invokeOrMock('set_translation_hotkey', { binding }, () => {
    mockSetSettings({
      ...mockSettings,
      translationHotkey: binding ?? { primary: '', modifiers: [] },
    });
  });
}

// binding = null disables (clears the global key), matching set_qa_hotkey (issue #576).
export function setSwitchStyleHotkey(binding: ShortcutBinding | null): Promise<void> {
  return invokeOrMock('set_switch_style_hotkey', { binding }, () => undefined);
}

export function setOpenAppHotkey(binding: ShortcutBinding | null): Promise<void> {
  return invokeOrMock('set_open_app_hotkey', { binding }, () => undefined);
}

export function setQuickNoteHotkey(binding: ShortcutBinding | null): Promise<void> {
  return invokeOrMock('set_quick_note_hotkey', { binding }, () => {
    mockSetSettings({ ...mockSettings, quickNoteHotkey: binding });
    return undefined;
  });
}

// Style pack direct hotkeys: whole-table replacement (any frontend add/remove/change sends the full list, issue #759).
export function setStylePackHotkeys(hotkeys: StylePackHotkey[]): Promise<void> {
  return invokeOrMock('set_style_pack_hotkeys', { hotkeys }, () => {
    mockSetSettings({ ...mockSettings, stylePackHotkeys: hotkeys });
    return undefined;
  });
}

export function setShortcutRecordingActive(active: boolean): Promise<void> {
  return invokeOrMock('set_shortcut_recording_active', { active }, () => undefined);
}
