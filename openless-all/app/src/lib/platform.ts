// Platform capability detection for desktop vs Android APK targets.
// Prefers Tauri `get_platform_capabilities`; falls back to UA / OS heuristics.

import { detectOS } from '../components/WindowChrome';
import type { PlatformCapabilities, PlatformKind } from './types';

export type { PlatformCapabilities, PlatformKind };

let cachedCapabilities: PlatformCapabilities | null = null;

function detectAndroidFromUa(): boolean {
  if (typeof navigator === 'undefined') return false;
  const uaDataPlatform =
    (navigator as Navigator & { userAgentData?: { platform?: string } })
      .userAgentData?.platform ?? '';
  const hints = `${navigator.userAgent || ''} ${navigator.platform || ''} ${uaDataPlatform}`;
  return /Android/i.test(hints);
}

function detectIosFromUa(): boolean {
  if (typeof navigator === 'undefined') return false;
  const uaDataPlatform =
    (navigator as Navigator & { userAgentData?: { platform?: string } })
      .userAgentData?.platform ?? '';
  const hints = `${navigator.userAgent || ''} ${navigator.platform || ''} ${uaDataPlatform}`;
  return /iPhone|iPad|iPod/i.test(hints);
}

/** Unavailable capability flags for iOS / other non-Android mobile targets. */
const MOBILE_UNAVAILABLE: PlatformCapabilities = {
  platform: 'mobile',
  supportsDesktopHotkey: false,
  supportsTray: false,
  supportsOverlay: false,
  supportsImeInput: false,
  supportsLocalAsr: false,
  supportsInAppDictation: false,
  supportsAutoUpdate: false,
};

export function isAndroid(): boolean {
  if (cachedCapabilities) return cachedCapabilities.platform === 'android';
  return detectOS() === 'android' || detectAndroidFromUa();
}

export function isMobile(): boolean {
  if (cachedCapabilities) {
    return (
      cachedCapabilities.platform === 'mobile' ||
      cachedCapabilities.platform === 'android'
    );
  }
  return isAndroid() || detectIosFromUa();
}

export function isDesktop(): boolean {
  if (cachedCapabilities) return cachedCapabilities.platform === 'desktop';
  return !isMobile();
}

export function inferPlatformCapabilities(): PlatformCapabilities {
  if (isAndroid()) {
    return {
      platform: 'android',
      supportsDesktopHotkey: false,
      supportsTray: false,
      supportsOverlay: true,
      supportsImeInput: false,
      supportsLocalAsr: false,
      supportsInAppDictation: true,
      supportsAutoUpdate: true,
    };
  }

  if (detectIosFromUa()) {
    return MOBILE_UNAVAILABLE;
  }

  const os = detectOS();
  return {
    platform: 'desktop',
    supportsDesktopHotkey: true,
    supportsTray: true,
    supportsOverlay: true,
    supportsImeInput: os === 'win',
    supportsLocalAsr: os === 'mac' || os === 'win',
    supportsInAppDictation: false,
    supportsAutoUpdate: true,
  };
}

export async function getPlatformCapabilities(): Promise<PlatformCapabilities> {
  if (cachedCapabilities) return cachedCapabilities;

  const isTauri =
    globalThis.window !== undefined &&
    '__TAURI_INTERNALS__' in globalThis.window;

  if (!isTauri) {
    cachedCapabilities = inferPlatformCapabilities();
    return cachedCapabilities;
  }

  try {
    const { invoke } = await import('@tauri-apps/api/core');
    cachedCapabilities = await invoke<PlatformCapabilities>(
      'get_platform_capabilities',
    );
    return cachedCapabilities;
  } catch (err) {
    console.warn(
      '[platform] get_platform_capabilities unavailable; using inferred defaults',
      err,
    );
    cachedCapabilities = inferPlatformCapabilities();
    return cachedCapabilities;
  }
}

export function getCachedPlatformCapabilities(): PlatformCapabilities | null {
  return cachedCapabilities;
}

/** Sync Windows 11 native title bar with in-app dark/light tokens. No-op off Windows. */
export async function syncWindowsCaptionTheme(dark: boolean): Promise<void> {
  if (detectOS() !== 'win') return;
  if (
    globalThis.window === undefined ||
    !('__TAURI_INTERNALS__' in globalThis.window)
  ) {
    return;
  }
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('set_windows_caption_theme', { dark });
  } catch (error) {
    console.warn('[platform] set_windows_caption_theme failed', error);
  }
}
