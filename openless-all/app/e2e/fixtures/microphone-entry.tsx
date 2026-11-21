import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { mockSettings } from '../../src/lib/ipc/mock-data';
import { getPreviewProfile } from '../../src/preview/profiles';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const profile = getPreviewProfile('mac');
const calls: string[] = [];
let status = 'granted';
let releaseCapabilities!: () => void;
const capabilities = new Promise<void>((resolve) => {
  releaseCapabilities = resolve;
});
let releasePermission: (() => void) | undefined;
let deferPermission = false;
const fixture = {
  calls,
  releaseCapabilities,
  setStatus: (next: string) => {
    status = next;
  },
  deferPermission: () => {
    deferPermission = true;
  },
  releasePermission: () => {
    releasePermission?.();
    releasePermission = undefined;
  },
};
export type MicrophoneEntryFixture = typeof fixture;
Object.assign(window, { microphoneEntryFixture: fixture });
localStorage.setItem('ol.locale', 'en');
mockWindows('main');
mockIPC(
  async (command) => {
    calls.push(command);
    switch (command) {
      case 'get_platform_capabilities':
        await capabilities;
        return profile.capabilities;
      case 'get_hotkey_capability':
        return profile.hotkeyCapability;
      case 'get_settings_snapshot':
        return { preferences: mockSettings, revision: 1 };
      case 'get_startup_snapshot':
        return { backend: { running: true }, contractVersion: '2.0.0' };
      case 'check_microphone_permission': {
        const snapshot = status;
        if (deferPermission) {
          deferPermission = false;
          await new Promise<void>((resolve) => {
            releasePermission = resolve;
          });
        }
        return snapshot;
      }
      case 'request_microphone_permission':
        return status;
      case 'list_microphone_devices':
        return [{ name: 'Granted microphone', isDefault: true }];
      case 'open_system_settings':
      case 'stop_microphone_level_monitor':
      case 'set_remote_locale':
      case 'plugin:app|set_app_theme':
        return;
      case 'experimental_asr_status':
        return [];
      default:
        throw new Error(`Unexpected fixture command: ${command}`);
    }
  },
  { shouldMockEvents: true },
);
const { i18nReady } = await import('../../src/i18n');
const { HotkeySettingsProvider } = await import('../../src/data/preferences');
const { SettingsModal } = await import('../../src/features/settings/shell/SettingsModal');
const { AppViewport } = await import('../../src/ui/layout/AppViewport');
const { InteractionProvider } = await import('../../src/ui/overlays/InteractionProvider');
const { TooltipProvider } = await import('../../src/ui/overlays/Tooltip');
await i18nReady;
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppViewport>
      <InteractionProvider>
        <TooltipProvider>
          <HotkeySettingsProvider>
            <SettingsModal os="mac" onClose={() => undefined} />
          </HotkeySettingsProvider>
        </TooltipProvider>
      </InteractionProvider>
    </AppViewport>
  </StrictMode>,
);
