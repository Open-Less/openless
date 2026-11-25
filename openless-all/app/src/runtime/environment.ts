export type OS = 'mac' | 'win' | 'android' | 'harmony';
export type RuntimeHost = 'preview' | 'tauri' | 'harmony';
export interface ShortcutPlatform {
  isMac: boolean;
  isWindows: boolean;
}
export interface RuntimeEnvironment {
  host: RuntimeHost;
  os: OS;
  isMobile: boolean;
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

interface EnvironmentHints {
  buildPlatform?: string;
  tauriPlatform?: string;
  protocol?: string;
  hostname?: string;
  search?: string;
  userAgent?: string;
  platform?: string;
  hasTauri?: boolean;
  hasHarmony?: boolean;
}

function parsePreviewOS(search: string | undefined): OS | undefined {
  const requested = new URLSearchParams(search).get('os');
  switch (requested) {
    case 'mac':
    case 'win':
    case 'android':
    case 'harmony':
      return requested;
    default:
      return undefined;
  }
}

export function resolveRuntimeEnvironment(hints: EnvironmentHints): RuntimeEnvironment {
  const { protocol, hostname, tauriPlatform, buildPlatform, hasHarmony, hasTauri } = hints;
  if (
    buildPlatform === 'harmony' ||
    hasHarmony ||
    (protocol === 'resource:' && hostname === 'rawfile')
  ) {
    return { host: 'harmony', os: 'harmony', isMobile: true };
  }

  // The origin or native Vite build identifies a WebView before Wry installs its bridge.
  const nativeOrigin =
    protocol === 'tauri:' ||
    ((protocol === 'http:' || protocol === 'https:') && hostname === 'tauri.localhost');
  const host = hasTauri || nativeOrigin || tauriPlatform ? 'tauri' : 'preview';
  const text = `${hints.userAgent ?? ''} ${hints.platform ?? ''}`;
  const isIos = tauriPlatform === 'ios' || /iPhone|iPad|iPod/i.test(text);
  const nativeOS =
    tauriPlatform === 'android' || tauriPlatform === 'androideabi' || /Android/i.test(text)
      ? 'android'
      : tauriPlatform === 'windows' || /Windows|Win32|Win64/i.test(text)
        ? 'win'
        : 'mac';
  const override = host === 'preview' ? parsePreviewOS(hints.search) : undefined;
  const os = override ?? nativeOS;
  return { host, os, isMobile: os === 'android' || os === 'harmony' || (!override && isIos) };
}

export function getRuntimeEnvironment(): RuntimeEnvironment {
  const window = globalThis.window;
  const nav = globalThis.navigator as
    (Navigator & { userAgentData?: { platform?: string } }) | undefined;
  return resolveRuntimeEnvironment({
    buildPlatform: import.meta.env?.VITE_OPENLESS_PLATFORM,
    tauriPlatform: import.meta.env?.TAURI_ENV_PLATFORM,
    protocol: window?.location?.protocol,
    hostname: window?.location?.hostname,
    search: window?.location?.search,
    userAgent: nav?.userAgent,
    platform: `${nav?.platform ?? ''} ${nav?.userAgentData?.platform ?? ''}`,
    hasTauri: window !== undefined && '__TAURI_INTERNALS__' in window,
    hasHarmony: window !== undefined && 'OpenLessHarmonyApi' in window,
  });
}

export function getRuntimeOS(): OS {
  return getRuntimeEnvironment().os;
}

export function isPreviewHost(): boolean {
  return getRuntimeEnvironment().host === 'preview';
}

export function isTauriHost(): boolean {
  return getRuntimeEnvironment().host === 'tauri';
}

export function isHarmonyRuntime(): boolean {
  return getRuntimeEnvironment().host === 'harmony';
}

export function shortcutPlatformFor(environment: RuntimeEnvironment): ShortcutPlatform {
  return {
    isMac: environment.os === 'mac' && !environment.isMobile,
    isWindows: environment.os === 'win',
  };
}

export function getShortcutPlatform(): ShortcutPlatform {
  return shortcutPlatformFor(getRuntimeEnvironment());
}
