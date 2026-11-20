import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { createRoot } from 'react-dom/client';
import type { AskEntrySettings } from '../../src/features/settings/sections/askEntrySettings';
import { mockSettings } from '../../src/lib/ipc/mock-data';
import { getPreviewProfile } from '../../src/preview/profiles';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const params = new URLSearchParams(location.search);
const settingsKey = 'fixture.askEntrySettings';
const writes: AskEntrySettings[] = [];
let finishWrite: ((failed: boolean) => void) | undefined;
const fixture = {
  writes,
  hasPendingWrite: () => finishWrite !== undefined,
  finish(failed: boolean) {
    if (!finishWrite) throw new Error('No pending Ask settings write');
    const finish = finishWrite;
    finishWrite = undefined;
    finish(failed);
  },
};
export type AskEntrySettingsFixture = typeof fixture;
Object.assign(window, { askEntrySettingsFixture: fixture });

localStorage.setItem('ol.locale', 'en');
const profile = getPreviewProfile('mac');
mockWindows('main');
mockIPC(
  (command, args) => {
    switch (command) {
      case 'get_startup_snapshot':
        return { backend: { running: true }, contractVersion: '2.0.0' };
      case 'get_platform_capabilities':
        return profile.capabilities;
      case 'get_hotkey_capability':
        return profile.hotkeyCapability;
      case 'get_settings_snapshot':
        return { preferences: { ...mockSettings, qaSaveHistory: true }, revision: 1 };
      case 'set_remote_locale':
      case 'plugin:app|set_app_theme':
        return undefined;
      case 'experimental_asr_status':
        return [];
      case 'ask_get_context_permission':
        return true;
      case 'ask_get_entry_settings': {
        const stored = localStorage.getItem(settingsKey);
        if (!stored && params.has('failRead')) throw new Error('Fixture storage is not readable');
        return stored
          ? JSON.parse(stored)
          : { autoVoiceOnOpen: true, intentMode: 'semanticHybrid' };
      }
      case 'ask_set_entry_settings': {
        const { settings } = args as { settings: AskEntrySettings };
        writes.push(settings);
        const commit = () => {
          localStorage.setItem(settingsKey, JSON.stringify(settings));
          return settings;
        };
        if (!params.has('defer')) return commit();
        return new Promise((resolve, reject) => {
          finishWrite = (failed) =>
            failed ? reject(new Error('Fixture storage is not writable')) : resolve(commit());
        });
      }
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
  <AppViewport>
    <InteractionProvider>
      <TooltipProvider>
        <HotkeySettingsProvider>
          <SettingsModal os="mac" onClose={() => undefined} />
        </HotkeySettingsProvider>
      </TooltipProvider>
    </InteractionProvider>
  </AppViewport>,
);
