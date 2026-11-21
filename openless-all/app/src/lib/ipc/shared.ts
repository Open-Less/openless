import type { HotkeyCapability, HotkeyStatus, WindowsImeStatus } from '../types';
import { isTauriHost } from '../../runtime/environment';

export {
  BACKEND_CONTRACT_VERSION,
  requireBackendReady,
  validateStartupSnapshot,
  invokeOrPreview as invokeOrMock,
  type StartupSnapshot,
} from '../../runtime/transport';
export {
  getPlatformCapabilities,
  getPlatformCapabilities as platformCapabilities,
} from '../../runtime/platform';

// Compatibility for older screens; the runtime checks native host signals on each call.
export const isTauri = isTauriHost();
export const isTauriNow = isTauriHost;

export const androidHotkeyCapability: HotkeyCapability = {
  adapter: 'unavailable',
  availableTriggers: [],
  requiresAccessibilityPermission: false,
  supportsModifierOnlyTrigger: false,
  supportsSideSpecificModifiers: false,
  explicitFallbackAvailable: false,
  statusHint: '移动端不支持全局热键；请使用应用内录音按钮或悬浮窗（需授权）。',
};

export const androidHotkeyStatus: HotkeyStatus = {
  adapter: 'unavailable',
  state: 'failed',
  message: '移动端不支持全局热键',
  lastError: {
    code: 'unavailable',
    message: 'Global hotkeys are not available on mobile',
  },
};

export const androidWindowsImeStatus: WindowsImeStatus = {
  state: 'notWindows',
  usingTsfBackend: false,
  message: 'Not available on Android',
  dllPath: null,
};
