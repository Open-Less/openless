import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';

import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';

let prefs: typeof import('../../src/lib/ipc/mock-data').mockSettings;
let mockHotkeyCapability: typeof import('../../src/lib/ipc/mock-data').mockHotkeyCapability;
let mockCredentialsStatus: typeof import('../../src/lib/ipc/mock-data').mockCredentialsStatus;
const calls: string[] = [];
const fixture = {
  microphone: 'denied',
  accessibility: 'denied',
  serviceReady: false,
  failSave: false,
  failPermission: false,
  openedPanes: [] as string[],
  calls,
};
Object.assign(window, {
  guideFixture: fixture,
  __TAURI_INTERNALS__: {
    transformCallback: () => 1,
    invoke: async (command: string, args: Record<string, any> = {}) => {
      calls.push(command);
      if (command === 'get_startup_snapshot')
        return { contractVersion: '2.0.0', backend: { running: true } };
      if (command === 'get_settings') return prefs;
      if (command === 'get_settings_snapshot') return { preferences: prefs, revision: 0 };
      if (command === 'get_platform_capabilities')
        return { platform: 'desktop', supportsDesktopHotkey: true };
      if (command === 'get_hotkey_capability') return mockHotkeyCapability;
      if (command === 'check_microphone_permission') {
        if (fixture.failPermission) throw Error('Permission probe unavailable');
        return fixture.microphone;
      }
      if (command === 'check_accessibility_permission') return fixture.accessibility;
      if (command === 'open_system_settings') {
        fixture.openedPanes.push(args.pane);
        return;
      }
      if (command === 'get_credentials')
        return {
          ...mockCredentialsStatus,
          asrConfigured: fixture.serviceReady,
          llmConfigured: fixture.serviceReady,
          omniConfigured: fixture.serviceReady,
        };
      if (command === 'update_setting_fields') {
        if (fixture.failSave) throw Error('Write unavailable');
        for (const [path, value] of Object.entries(args.edits)) {
          (prefs as any)[path.slice(1)] = value;
        }
        return { preferences: prefs, revision: 1 };
      }
      return null;
    },
  },
});
const mocks = await import('../../src/lib/ipc/mock-data');
prefs = structuredClone(mocks.mockSettings);
mockHotkeyCapability = mocks.mockHotkeyCapability;
mockCredentialsStatus = mocks.mockCredentialsStatus;
const [{ DesktopOnboarding }, { HotkeySettingsProvider }, { i18nReady }] = await Promise.all([
  import('../../src/components/DesktopOnboarding'),
  import('../../src/state/HotkeySettingsContext'),
  import('../../src/i18n'),
]);
await i18nReady;
function Fixture() {
  const [completed, setCompleted] = useState(false);
  return (
    <HotkeySettingsProvider>
      <div style={{ width: '100%', height: '100%' }}>
        {completed ? (
          <h1>Guide completed</h1>
        ) : (
          <DesktopOnboarding onComplete={() => setCompleted(true)} />
        )}
      </div>
    </HotkeySettingsProvider>
  );
}
createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Fixture />
  </React.StrictMode>,
);
