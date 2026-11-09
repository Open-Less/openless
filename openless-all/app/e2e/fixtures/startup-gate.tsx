import React from 'react';
import { createRoot } from 'react-dom/client';
import type { CredentialsStatus } from '../../src/lib/types';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';

const params = new URLSearchParams(location.search);
const mode = params.get('credentials');
const calls: string[] = [];
const pending: { resolve: (value: CredentialsStatus) => void; reject: (error: Error) => void }[] =
  [];
let mocks: typeof import('../../src/lib/ipc/mock-data');
const status = (configured: boolean) => ({
  ...mocks.mockCredentialsStatus,
  asrConfigured: configured,
  llmConfigured: configured,
  omniConfigured: configured,
});

Object.assign(window, {
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
  startupFixture: {
    calls,
    resolve: () => pending.splice(0).forEach(({ resolve }) => resolve(status(false))),
    reject: () =>
      pending.splice(0).forEach(({ reject }) => reject(new Error('Late store failure'))),
  },
  __TAURI_INTERNALS__: {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    transformCallback: () => 1,
    invoke: async (command: string) => {
      calls.push(command);
      if (command === 'get_startup_snapshot')
        return {
          contractVersion: params.has('badHandshake') ? 'invalid' : '2.0.0',
          backend: { running: true },
        };
      if (command === 'get_platform_capabilities')
        return { platform: 'desktop', supportsDesktopHotkey: true };
      if (command === 'get_settings') return mocks.mockSettings;
      if (command === 'get_settings_snapshot')
        return { preferences: mocks.mockSettings, revision: 0 };
      if (command === 'get_hotkey_capability') return mocks.mockHotkeyCapability;
      if (command === 'get_hotkey_status') return { state: 'ready' };
      if (command === 'check_microphone_permission' || command === 'check_accessibility_permission')
        return 'granted';
      if (command === 'plugin:window|is_visible') return true;
      if (command === 'get_credentials') {
        if (mode === 'reject') throw new Error('Store unavailable');
        if (mode === 'hang')
          return new Promise<CredentialsStatus>((resolve, reject) =>
            pending.push({ resolve, reject }),
          );
        return status(mode !== 'unconfigured');
      }
      return null;
    },
  },
});
mocks = await import('../../src/lib/ipc/mock-data');
const [{ App }, { i18nReady }] = await Promise.all([
  import('../../src/App'),
  import('../../src/i18n'),
]);
await i18nReady;
createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App
      isCapsule={false}
      isCapsuleRail={false}
      isQa={false}
      isSelectionVoiceIntent={false}
      isVoiceEdit={false}
      isLessComputer={false}
      isLessComputerGlow={false}
      forcedOs={params.get('os') === 'win' ? 'win' : 'mac'}
    />
  </React.StrictMode>,
);
