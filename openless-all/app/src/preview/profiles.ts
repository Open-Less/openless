import type {
  HotkeyCapability,
  HotkeyStatus,
  PlatformCapabilities,
  WindowsImeStatus,
} from '../api/contracts';
import {
  getRuntimeOS,
  shortcutPlatformFor,
  type OS,
  type ShortcutPlatform,
} from '../runtime/environment';

interface PreviewProfile {
  viewport: { width: number; height: number };
  capabilities: PlatformCapabilities;
  shortcutPlatform: ShortcutPlatform;
  hotkeyCapability: HotkeyCapability | null;
  hotkeyStatus: HotkeyStatus | null;
  windowsImeStatus: WindowsImeStatus;
}

export const unavailableMobileCapabilities: PlatformCapabilities = {
  platform: 'mobile',
  supportsDesktopHotkey: false,
  supportsTray: false,
  supportsOverlay: false,
  supportsImeInput: false,
  supportsLocalAsr: false,
  supportsLocalQwen3Mlx: false,
  supportsInAppDictation: false,
  supportsAutoUpdate: false,
  supportsVoiceEdit: false,
};

const viewports: Record<OS, PreviewProfile['viewport']> = {
  mac: { width: 1400, height: 900 },
  win: { width: 1240, height: 800 },
  android: { width: 412, height: 892 },
  harmony: { width: 420, height: 900 },
};

export function getPreviewProfile(os: OS = getRuntimeOS()): PreviewProfile {
  const desktop = os === 'mac' || os === 'win';
  const windows = os === 'win';
  const adapter = windows ? 'windowsLowLevel' : 'macEventTap';
  const capabilities: PlatformCapabilities = desktop
    ? {
        platform: 'desktop',
        supportsDesktopHotkey: true,
        supportsTray: true,
        supportsOverlay: true,
        supportsImeInput: windows,
        supportsLocalAsr: true,
        supportsLocalQwen3Mlx: !windows,
        supportsInAppDictation: false,
        supportsAutoUpdate: true,
        supportsVoiceEdit: true,
      }
    : {
        ...unavailableMobileCapabilities,
        platform: os,
        supportsInAppDictation: true,
        supportsOverlay: os === 'android',
        supportsAutoUpdate: os === 'android',
        supportsVoiceEdit: os === 'android',
      };
  return {
    viewport: { ...viewports[os] },
    capabilities,
    shortcutPlatform: shortcutPlatformFor({ host: 'preview', os, isMobile: !desktop }),
    hotkeyCapability: desktop
      ? {
          adapter,
          availableTriggers: windows
            ? [
                'rightControl',
                'rightAlt',
                'leftControl',
                'leftShift',
                'rightShift',
                'mediaPlayPause',
                'custom',
              ]
            : [
                'rightControl',
                'rightOption',
                'leftOption',
                'leftControl',
                'rightCommand',
                'leftCommand',
                'leftShift',
                'rightShift',
                'fn',
                'custom',
              ],
          requiresAccessibilityPermission: !windows,
          supportsModifierOnlyTrigger: true,
          supportsSideSpecificModifiers: true,
          explicitFallbackAvailable: false,
          statusHint: '',
        }
      : null,
    hotkeyStatus: desktop
      ? { adapter, state: 'installed', message: 'Browser preview', lastError: null }
      : null,
    windowsImeStatus: {
      state: windows ? 'installed' : 'notWindows',
      usingTsfBackend: windows,
      message: 'Browser preview',
      dllPath: null,
    },
  };
}
