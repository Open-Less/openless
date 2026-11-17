import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import type { Channel } from '../../src/api/providers';
import type { PresetOption } from '../../src/features/providers/channelCatalog';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';

const calls: string[] = [];
const credentials = new Map<string, string>();
let disposition: 'accept' | 'reject' | 'hold' = 'accept';
let settle: (() => void) | undefined;
const fixture = {
  calls,
  reject: () => {
    disposition = 'reject';
  },
  hold: () => {
    disposition = 'hold';
  },
  release: () => {
    disposition = 'accept';
    settle?.();
  },
};
let mockSettings: Awaited<ReturnType<typeof import('../../src/api/preferences').getSettings>>;
let getPreviewProfile: typeof import('../../src/preview/profiles').getPreviewProfile;
Object.assign(window, {
  channelFixture: fixture,
  __TAURI_INTERNALS__: {
    transformCallback: () => 1,
    async invoke(command: string, args: Record<string, string> = {}) {
      if (command === 'get_startup_snapshot') {
        return { contractVersion: '2.0.0', backend: { running: true } };
      }
      if (command === 'get_settings_snapshot') return { revision: 0, preferences: mockSettings };
      if (command === 'get_platform_capabilities') return getPreviewProfile('mac').capabilities;
      if (command === 'get_hotkey_capability') return getPreviewProfile('mac').hotkeyCapability;
      if (command === 'read_credential') return credentials.get(args.account) ?? null;
      if (command === 'set_credential') {
        if (args.account.endsWith('api_key')) {
          calls.push('credential-start');
          if (disposition === 'hold')
            await new Promise<void>((resolve) => {
              settle = resolve;
            });
          if (disposition === 'reject') {
            calls.push('credential-rejected');
            throw new Error('Fixture keychain refused the write');
          }
          calls.push('credential-committed');
        }
        credentials.set(args.account, args.value);
      }
      if (command === 'set_channel_provider_type') calls.push(`provider:${args.providerType}`);
      return null;
    },
  },
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
});

const [
  channelModule,
  preferenceModule,
  viewportModule,
  translationModule,
  profiles,
  mocks,
  interactionModule,
] = await Promise.all([
  import('../../src/features/providers/ChannelEditor'),
  import('../../src/data/preferences'),
  import('../../src/ui/layout/AppViewport'),
  import('../../src/i18n'),
  import('../../src/preview/profiles'),
  import('../../src/lib/ipc/mock-data'),
  import('../../src/ui/overlays/InteractionProvider'),
]);
getPreviewProfile = profiles.getPreviewProfile;
mockSettings = structuredClone(mocks.mockSettings);
await translationModule.i18nReady;
const { ChannelEditor } = channelModule;
const { HotkeySettingsProvider } = preferenceModule;
const { AppViewport } = viewportModule;
const { InteractionProvider } = interactionModule;

const channel: Channel = {
  id: 'fixture-channel',
  name: 'Fixture channel',
  order: 0,
  providerType: 'custom',
  enabled: true,
  lastTest: null,
};
const presets: PresetOption[] = [
  {
    id: 'custom',
    nameKey: 'custom',
    authRequirement: 'api_key',
    defaultRequestFormat: 'messages',
    supportedRequestFormats: ['messages'],
  },
  { id: 'deepseek', nameKey: 'deepseek', authRequirement: 'api_key' },
];
function Fixture() {
  const [open, setOpen] = useState(true);
  Object.assign(fixture, { forceClose: () => setOpen(false) });
  return (
    <HotkeySettingsProvider>
      <AppViewport>
        <InteractionProvider>
          {open ? (
            <ChannelEditor
              kind="llm"
              channel={channel}
              presets={presets}
              isDraft={false}
              mobile={false}
              onClose={() => setOpen(false)}
              onChanged={() => {
                calls.push('refreshed');
              }}
              onUserMutation={() => {}}
            />
          ) : (
            <p>Editor closed</p>
          )}
        </InteractionProvider>
      </AppViewport>
    </HotkeySettingsProvider>
  );
}
createRoot(document.getElementById('root')!).render(<Fixture />);
