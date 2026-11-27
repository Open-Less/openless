import { supportsSkipAutoStyleHotkey } from './skipAutoStyleSettings';

const desktop = { platform: 'desktop', supportsDesktopHotkey: true } as const;
for (const adapter of ['macEventTap', 'windowsLowLevel'] as const) {
  if (!supportsSkipAutoStyleHotkey(desktop, adapter)) {
    throw new Error(`Expected skip-auto-style shortcuts for ${adapter}`);
  }
  if (
    supportsSkipAutoStyleHotkey({ ...desktop, supportsDesktopHotkey: false }, adapter) ||
    supportsSkipAutoStyleHotkey({ ...desktop, platform: 'mobile' }, adapter)
  ) {
    throw new Error(`Expected a desktop capability for ${adapter}`);
  }
}

for (const adapter of ['fcitx5', 'unavailable', undefined] as const) {
  if (supportsSkipAutoStyleHotkey(desktop, adapter)) {
    throw new Error(`Unexpected skip-auto-style shortcuts for ${adapter}`);
  }
}
