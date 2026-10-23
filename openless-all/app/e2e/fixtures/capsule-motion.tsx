// Real production components with an isolated native event/geometry adapter.
import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import type { CapsuleState } from '../../src/lib/types';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';

const callbacks = new Map<number, (value: unknown) => void>();
const listeners = new Map<string, Set<number>>();
let callbackId = 0;
const qa = new URLSearchParams(location.search).has('qa');
const bridge = {
  emit(event: string, payload: unknown) {
    for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
  },
  hasListener: (event: string) => Boolean(listeners.get(event)?.size),
  setCapsule: (_: Record<string, unknown>) => {},
  sizes: [] as boolean[],
  target: {
    sessionId: 'recording-a',
    targetLanguage: 'English',
    workingLanguages: ['简体中文'],
    mutable: true,
  },
  targetChanges: [] as any[],
  menuChanges: [] as any[],
  menuOwner: null as string | null,
  failTarget: false,
  delayTarget: 0,
};
Object.assign(window, { fixture: bridge });
window.__TAURI_INTERNALS__ = {
  transformCallback(callback: (value: unknown) => void) {
    const id = ++callbackId;
    callbacks.set(id, callback);
    return id;
  },
  unregisterCallback(id: number) {
    callbacks.delete(id);
  },
  async invoke(command: string, args: any) {
    if (command === 'plugin:event|listen') {
      const ids = listeners.get(args.event) ?? new Set<number>();
      ids.add(args.handler);
      listeners.set(args.event, ids);
      return args.handler;
    }
    if (command === 'plugin:event|unlisten') {
      listeners.get(args.event)?.delete(args.eventId);
      return;
    }
    if (command === 'get_startup_snapshot')
      return { contractVersion: '2.0.0', backend: { running: true } };
    if (command === 'get_settings')
      return {
        translationTargetLanguage: bridge.target.targetLanguage,
        capsuleStyle: 'classic',
        capsuleTranscriptEnabled: false,
      };
    if (command === 'get_dictation_translation_target') return { ...bridge.target };
    if (command === 'set_capsule_language_menu') {
      if (args.open && (!bridge.target.mutable || args.sessionId !== bridge.target.sessionId))
        throw new Error('stale menu');
      if (!args.open && bridge.menuOwner !== args.sessionId) return;
      bridge.menuOwner = args.open ? args.sessionId : null;
      bridge.menuChanges.push(args);
      const resize = (window as any).resizeMenuForTest;
      if (resize) await resize(args.open);
      return;
    }
    if (command === 'select_dictation_translation_target') {
      bridge.targetChanges.push(args);
      if (bridge.delayTarget)
        await new Promise((resolve) => setTimeout(resolve, bridge.delayTarget));
      if (bridge.failTarget || args.sessionId !== bridge.target.sessionId || !bridge.target.mutable)
        throw new Error('stale or failed');
      bridge.target.targetLanguage = args.targetLanguage;
      bridge.emit('prefs:changed', {});
      return { ...bridge.target };
    }
    if (command === 'qa_get_snapshot') return { kind: 'idle', messages: [] };
    if (command === 'qa_window_set_expanded') {
      bridge.sizes.push(args.expanded);
      const host = document.querySelector<HTMLElement>('#native-host');
      if (host) {
        host.style.width = args.expanded ? '760px' : '480px';
        host.style.height = args.expanded ? '416px' : '80px';
        host.style.bottom = args.expanded ? '48px' : '8px';
      }
      return;
    }
    if (command === 'qa_window_dismiss') bridge.emit('chat-panel:closing', {});
    return null;
  },
};
Object.assign(window, { __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} } });
const { i18nReady } = await import('../../src/i18n');
await i18nReady;
const { OpenLessPill } = await import('../../src/components/OpenLessCapsule');
const { CapsuleTranslationBadge } = await import('../../src/components/LiveTranscriptPill');
const { QaPanel } = await import('../../src/pages/QaPanel');
const { Capsule } = await import('../../src/components/Capsule');
function CapsuleFixture() {
  const [state, set] = useState({
    state: 'recording' as CapsuleState,
    level: 0.2,
    warming: false,
    translation: false,
    leaving: false,
    operating: false,
  });
  bridge.setCapsule = (patch) => set((previous) => ({ ...previous, ...patch }));
  return (
    <div
      style={{
        position: 'absolute',
        bottom: 16,
        left: '50%',
        transform: 'translateX(-50%)',
        display: 'flex',
        alignItems: 'center',
        flexDirection: 'column',
        gap: 8,
      }}
    >
      <CapsuleTranslationBadge
        visible={state.translation && state.state === 'recording' && !state.leaving}
      />
      <OpenLessPill
        {...state}
        onCancel={() => bridge.setCapsule({ leaving: true })}
        onConfirm={() => bridge.setCapsule({ state: 'polishing' })}
      />
    </div>
  );
}
document.body.style.background = '#0b1215';
createRoot(document.getElementById('root')!).render(
  new URLSearchParams(location.search).has('full') ? (
    <div
      style={{
        position: 'absolute',
        width: 460,
        height: 172,
        bottom: 0,
        left: '50%',
        transform: 'translateX(-50%)',
      }}
    >
      <Capsule />
    </div>
  ) : qa ? (
    <div
      id="native-host"
      style={{
        position: 'absolute',
        width: 480,
        height: 80,
        bottom: 8,
        left: '50%',
        transform: 'translateX(-50%)',
      }}
    >
      <QaPanel />
    </div>
  ) : (
    <CapsuleFixture />
  ),
);
