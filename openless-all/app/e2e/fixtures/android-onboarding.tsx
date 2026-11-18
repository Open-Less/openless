import React, { useState } from 'react';
import { AppViewport } from '../../src/ui/layout/AppViewport';
import { InteractionProvider } from '../../src/ui/overlays/InteractionProvider';
import type { UserPreferences } from '../../src/api/contracts';
import { createRoot } from 'react-dom/client';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';
import '../../src/features/onboarding/desktop-onboarding.css';

let mocks: typeof import('../../src/lib/ipc/mock-data');
const calls: string[] = [];
const writes: Array<Record<string, unknown>> = [];
let settings: UserPreferences;
let revision = 0;
let rejectWrite = false;
Object.assign(window, {
  androidGuideFixture: {
    calls,
    writes,
    rejectNextWrite: () => {
      rejectWrite = true;
    },
  },
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
  __TAURI_INTERNALS__: {
    transformCallback: () => 1,
    invoke: async (command: string, args?: { edits?: Record<string, unknown> }) => {
      calls.push(command);
      if (command === 'get_startup_snapshot')
        return { contractVersion: '2.0.0', backend: { running: true } };
      if (command === 'get_platform_capabilities')
        return { platform: 'android', supportsDesktopHotkey: false, supportsLocalAsr: false };
      if (command === 'check_microphone_permission') return 'granted';
      if (command === 'get_android_overlay_status')
        return { permission: 'granted', overlayVisible: true, message: '' };
      if (command === 'get_android_accessibility_status')
        return { state: 'enabled', enabled: true, operational: true, messageKey: '' };
      if (command === 'get_android_shizuku_status')
        return {
          state: 'notInstalled',
          messageKey: 'not_installed',
          accessibility: { registered: true, operational: true, messageKey: '' },
        };
      if (command === 'get_settings') return structuredClone(settings);
      if (command === 'get_settings_snapshot')
        return { preferences: structuredClone(settings), revision };
      if (command === 'update_setting_fields') {
        const edits = args?.edits ?? {};
        writes.push(edits);
        if (rejectWrite) {
          rejectWrite = false;
          throw new Error('Fixture rejected the preference write');
        }
        for (const [path, value] of Object.entries(edits)) {
          const keys = path.slice(1).split('/');
          let target = settings as unknown as Record<string, unknown>;
          for (const key of keys.slice(0, -1)) target = target[key] as Record<string, unknown>;
          target[keys[keys.length - 1]] = value;
        }
        return { preferences: structuredClone(settings), revision: ++revision };
      }
      if (command === 'get_hotkey_capability') return mocks.mockHotkeyCapability;
      // Keep the real provider step populated so advancing does not open an editor.
      if (command === 'list_channels')
        return [
          {
            id: 'groq',
            name: 'Groq',
            providerType: 'groq',
            enabled: true,
            order: 0,
            lastTest: null,
          },
        ];
      if (command === 'list_provider_descriptors') return [];
      if (command === 'read_credential') return '';
      return null;
    },
  },
});
mocks = await import('../../src/lib/ipc/mock-data');
settings = structuredClone(mocks.mockSettings);
const [{ Onboarding }, { HotkeySettingsProvider }, { i18nReady }] = await Promise.all([
  import('../../src/components/Onboarding'),
  import('../../src/state/HotkeySettingsContext'),
  import('../../src/i18n'),
]);
await i18nReady;

function Fixture() {
  const [completed, setCompleted] = useState(false);
  return (
    <HotkeySettingsProvider>
      {/* Match MainWindow's containing block and layer. Import the production layer CSS
          above; a flex test wrapper would hide the broken flex: 1 sizing contract. */}
      <div style={{ width: '100%', height: '100%', minWidth: 0, position: 'relative' }}>
        {completed ? (
          <h1>Guide completed</h1>
        ) : (
          <div className="ol-onboarding-layer">
            <Onboarding onComplete={() => setCompleted(true)} />
          </div>
        )}
      </div>
    </HotkeySettingsProvider>
  );
}
createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AppViewport scale={false}>
      <InteractionProvider>
        <Fixture />
      </InteractionProvider>
    </AppViewport>
  </React.StrictMode>,
);
