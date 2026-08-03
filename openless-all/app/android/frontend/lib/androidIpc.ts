import { invokeOrMock } from '../../../src/lib/ipc';
import type {
  AndroidAccessibilityRecoveryResult,
  AndroidAccessibilityStatus,
  AndroidOverlayStatus,
  AndroidShizukuActionResult,
  AndroidShizukuStatus,
} from './androidTypes';

export function getAndroidOverlayStatus(): Promise<AndroidOverlayStatus> {
  return invokeOrMock('get_android_overlay_status', undefined, () => ({
    permission: 'notAndroid',
    overlayVisible: false,
    message: 'Android overlay is only available on Android',
  }));
}

export function requestAndroidOverlayPermission(): Promise<{ launched: boolean; message: string }> {
  return invokeOrMock('request_android_overlay_permission', undefined, () => ({
    launched: false,
    message: 'Mock: overlay permission unavailable in browser preview',
  }));
}

export function showAndroidOverlay(): Promise<void> {
  return invokeOrMock('show_android_overlay', undefined, () => undefined);
}

export function hideAndroidOverlay(): Promise<void> {
  return invokeOrMock('hide_android_overlay', undefined, () => undefined);
}

export function getAndroidAccessibilityStatus(): Promise<AndroidAccessibilityStatus> {
  return invokeOrMock('get_android_accessibility_status', undefined, () => ({
    state: 'notAndroid',
    enabled: false,
    operational: false,
    messageKey: 'not_android',
  }));
}

export function requestAndroidAccessibilityPermission(): Promise<{ launched: boolean; message: string }> {
  return invokeOrMock('request_android_accessibility_permission', undefined, () => ({
    launched: false,
    message: 'Mock: accessibility settings unavailable in browser preview',
  }));
}

export function getAndroidShizukuStatus(): Promise<AndroidShizukuStatus> {
  return invokeOrMock('get_android_shizuku_status', undefined, () => ({
    state: 'notAndroid',
    messageKey: 'not_android',
    accessibility: {
      registered: false,
      operational: false,
      messageKey: 'not_android',
    },
  }));
}

export function requestAndroidShizukuPermission(): Promise<AndroidShizukuActionResult> {
  return invokeOrMock('request_android_shizuku_permission', undefined, () => ({
    launched: false,
    messageKey: 'not_android',
  }));
}

export function openShizukuApp(): Promise<AndroidShizukuActionResult> {
  return invokeOrMock('open_shizuku_app', undefined, () => ({
    launched: false,
    messageKey: 'not_android',
  }));
}

export function recoverAndroidAccessibility(confirmed: boolean): Promise<AndroidAccessibilityRecoveryResult> {
  return invokeOrMock('recover_android_accessibility', { confirmed }, () => ({
    outcome: confirmed ? 'shizukuUnavailable' : 'userNotConfirmed',
    messageKey: confirmed ? 'not_android' : 'user_not_confirmed',
  }));
}
