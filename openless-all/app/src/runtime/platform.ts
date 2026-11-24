import { getPreviewProfile, unavailableMobileCapabilities } from '../preview/profiles';
import type { PlatformCapabilities, PlatformKind } from '../api/contracts';
import { getRuntimeEnvironment } from './environment';
import { invokeOrPreview } from './transport';

export type { PlatformCapabilities, PlatformKind };

let cachedCapabilities: PlatformCapabilities | null = null;
let pendingCapabilities: Promise<PlatformCapabilities> | null = null;

export function inferPlatformCapabilities(): PlatformCapabilities {
  const environment = getRuntimeEnvironment();
  if (environment.isMobile && environment.os === 'mac') return { ...unavailableMobileCapabilities };
  if (environment.host === 'preview') return getPreviewProfile(environment.os).capabilities;
  if (environment.os === 'harmony') {
    return { ...unavailableMobileCapabilities, platform: 'harmony', supportsInAppDictation: true };
  }
  if (environment.os === 'android') {
    return { ...unavailableMobileCapabilities, platform: 'android', supportsInAppDictation: true };
  }
  // Architecture-dependent capabilities are enabled only by the native handshake.
  return { ...unavailableMobileCapabilities, platform: 'desktop' };
}

export function getPlatformCapabilities(): Promise<PlatformCapabilities> {
  if (cachedCapabilities) return Promise.resolve(cachedCapabilities);
  pendingCapabilities ??= invokeOrPreview(
    'get_platform_capabilities',
    undefined,
    inferPlatformCapabilities,
  )
    .then((capabilities) => (cachedCapabilities = capabilities))
    .finally(() => (pendingCapabilities = null));
  return pendingCapabilities;
}

export function getCachedPlatformCapabilities(): PlatformCapabilities | null {
  return cachedCapabilities;
}

export function isAndroid(): boolean {
  return (cachedCapabilities?.platform ?? getRuntimeEnvironment().os) === 'android';
}

export function isMobile(): boolean {
  return cachedCapabilities
    ? cachedCapabilities.platform !== 'desktop'
    : getRuntimeEnvironment().isMobile;
}

export function isDesktop(): boolean {
  return !isMobile();
}
