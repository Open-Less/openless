import { invokeOrMock } from '../../../src/lib/ipc';
import type {
  AndroidAccessibilityStatus,
  AndroidOverlayStatus,
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
    message: 'Android accessibility is only available on Android',
  }));
}

export function requestAndroidAccessibilityPermission(): Promise<{ launched: boolean; message: string }> {
  return invokeOrMock('request_android_accessibility_permission', undefined, () => ({
    launched: false,
    message: 'Mock: accessibility settings unavailable in browser preview',
  }));
}
