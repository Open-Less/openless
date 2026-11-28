import type { HotkeyCapability, PlatformCapabilities } from '../../api/contracts';

export function supportsSkipAutoStyleHotkey(
  platform: Pick<PlatformCapabilities, 'platform' | 'supportsDesktopHotkey'>,
  adapter: HotkeyCapability['adapter'] | undefined,
) {
  return (
    platform.platform === 'desktop' &&
    platform.supportsDesktopHotkey &&
    (adapter === 'macEventTap' || adapter === 'windowsLowLevel')
  );
}
