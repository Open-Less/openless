import React from 'react';
import { createRoot } from 'react-dom/client';
import type { CredentialsStatus } from '../../src/api/contracts';
import type { VoiceEditSnapshot } from '../../src/api/voice-edit';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const params = new URLSearchParams(location.search);
const mode = params.get('credentials');
const android = params.get('os') === 'android';
const exerciseClipboard = params.has('clipboard');
let clipboardAllowed = false;
if (android) localStorage.setItem('openless.androidSetupWizardComplete', '1');
let voice: VoiceEditSnapshot | null = null;
const cancelRequests: Array<{ id?: string; resolve: () => void; reject: (reason: Error) => void }> =
  [];
const callbacks = new Map<number, (value: unknown) => void>();
const subscriptions = new Map<number, { event: string; callbackId: number }>();
let callbackId = 0;
let listenerId = 0;
const emit = (event: string, payload: unknown) => {
  for (const [id, listener] of subscriptions) {
    if (listener.event === event) callbacks.get(listener.callbackId)?.({ id, event, payload });
  }
};
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
  __TAURI_EVENT_PLUGIN_INTERNALS__: {
    unregisterListener: (_event: string, id: number) => subscriptions.delete(id),
  },
  startupFixture: {
    calls,
    allowClipboard: (allowed: boolean) => {
      clipboardAllowed = allowed;
    },
    emit,
    listening: (event: string) =>
      [...subscriptions.values()].some((listener) => listener.event === event),
    showVoice: (sessionId: string) => {
      voice = {
        sessionId,
        phase: 'preview',
        target: { kind: 'full_field' },
        context: {
          fieldText: '',
          dictatedSegment: '',
          draft: sessionId,
          preview: sessionId,
          turns: [],
        },
      };
      emit('voice-edit:show', {});
    },
    pendingCancels: () => cancelRequests.map((request) => request.id),
    finishCancel: (succeed: boolean) => {
      const request = cancelRequests.shift();
      if (!request) throw new Error('No pending voice dismissal');
      if (succeed) request.resolve();
      else request.reject(new Error('Microphone is still busy'));
    },
    resolve: () => pending.splice(0).forEach(({ resolve }) => resolve(status(false))),
    reject: () =>
      pending.splice(0).forEach(({ reject }) => reject(new Error('Late store failure'))),
  },
  __TAURI_INTERNALS__: {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    transformCallback: (callback: (value: unknown) => void) => {
      const id = ++callbackId;
      callbacks.set(id, callback);
      return id;
    },
    invoke: async (
      command: string,
      args?: { event?: string; handler?: number; sessionId?: string },
    ) => {
      calls.push(command);
      if (command === 'get_startup_snapshot')
        return {
          contractVersion: params.has('badHandshake') ? 'invalid' : '2.0.0',
          backend: { running: true },
        };
      if (command === 'get_platform_capabilities')
        return { platform: android ? 'android' : 'desktop', supportsDesktopHotkey: !android };
      if (command === 'plugin:event|listen') {
        const id = ++listenerId;
        subscriptions.set(id, { event: args!.event!, callbackId: args!.handler! });
        return id;
      }
      if (command === 'get_voice_edit_state') return voice;
      if (command === 'qa_get_snapshot') return { kind: 'idle', messages: [] };
      if (command === 'qa_window_dismiss') {
        emit('qa:dismiss', {});
        return;
      }
      if (command === 'cancel_voice_edit_session') {
        const id = args?.sessionId;
        await new Promise<void>((resolve, reject) => cancelRequests.push({ id, resolve, reject }));
        if (voice && voice.sessionId === id) voice = null;
        return null;
      }
      if (command === 'get_settings')
        return { ...mocks.mockSettings, remoteInputEnabled: exerciseClipboard };
      if (command === 'get_settings_snapshot')
        return {
          preferences: { ...mocks.mockSettings, remoteInputEnabled: exerciseClipboard },
          revision: 0,
        };
      if (exerciseClipboard && command === 'get_remote_input_status')
        return {
          running: true,
          starting: false,
          port: 8443,
          urlsStale: false,
          urls: ['https://192.0.2.10:8443'],
          pin: '000001',
          caFingerprintSha256: 'a'.repeat(64),
        };
      if (exerciseClipboard && command === 'copy_text_to_clipboard') {
        if (!clipboardAllowed) throw new Error('Clipboard write rejected');
        return;
      }
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
  import('../../src/app/App'),
  import('../../src/i18n'),
]);
await i18nReady;
createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App kind="main" os={android ? 'android' : params.get('os') === 'win' ? 'win' : 'mac'} />
  </React.StrictMode>,
);
