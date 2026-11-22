// Real production components with an isolated native event/geometry adapter.
import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import type { CapsuleState, ThemeMode } from '../../src/lib/types';
import type { QaStatePayload } from '../../src/api/contracts';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import { AppViewport } from '../../src/ui/layout/AppViewport';

const callbacks = new Map<number, (value: unknown) => void>();
const listeners = new Map<string, Set<number>>();
const listenerTargets = new Map<number, { kind: string; label?: string }>();
let callbackId = 0;
const qa = new URLSearchParams(location.search).has('qa');
const params = new URLSearchParams(location.search);
const windowLabel = params.get('label') ?? (qa ? 'qa' : 'capsule');
const themeSync = params.has('themeSync');
const holdThemeRead = params.has('holdThemeRead');
let releaseThemeRead: (() => void) | undefined;
let themeReadStarted!: () => void;
const firstThemeRead = new Promise<void>((resolve) => {
  themeReadStarted = resolve;
});
let qaSnapshot: QaStatePayload = { kind: 'idle', messages: [] };
const bridge = {
  emit(event: string, payload: unknown) {
    if (event === 'chat-panel:closing') qaSnapshot = { kind: 'idle', messages: [] };
    if (event === 'qa:state') {
      const state = payload as QaStatePayload;
      // Stream chunks, activity and the host's lightweight idle are not snapshots.
      if (state.kind !== 'answer_delta' && state.kind !== 'activity' && state.messages)
        qaSnapshot = state;
    }
    for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
  },
  emitTo(label: string, event: string, payload: unknown) {
    for (const id of listeners.get(event) ?? []) {
      const target = listenerTargets.get(id);
      if (!target || target.kind === 'Any' || target.label === label)
        callbacks.get(id)?.({ event, id, payload });
    }
  },
  transcriptVisibility: [] as boolean[],
  inputRegions: [] as any[],
  regionWrites: [] as any[],
  dictationActions: [] as string[],
  qaActions: [] as { command: string; args: unknown }[],
  pipelineMode: params.get('pipelineMode') === 'deepseek' ? 'deepseek' : 'traditional',
  hasListener: (event: string) => Boolean(listeners.get(event)?.size),
  setCapsule: (_: Record<string, unknown>) => {},
  theme: 'light' as ThemeMode,
  themeReads: 0,
  nativeThemeWrites: [] as unknown[],
  releaseThemeRead: () => releaseThemeRead?.(),
  setTheme(themeMode: ThemeMode) {
    bridge.theme = themeMode;
    bridge.emit('prefs:changed', { themeMode });
  },
  sizes: [] as boolean[],
  holdQaGeometry: false,
  fallbackReports: [] as { presentationId: number; height: number }[],
  copiedFallback: [] as string[],
  rejectQaExpansion: false,
  applyQaGeometry(expanded: boolean, width?: number, height?: number) {
    const host = document.querySelector<HTMLElement>('#native-host');
    if (!host) return;
    // Match the native window's nominal dimensions; tests can deliver a work-area clamp.
    host.style.width = `${width ?? (expanded ? 720 : 480)}px`;
    host.style.height = `${height ?? (expanded ? 380 : 80)}px`;
    host.style.bottom = expanded ? '48px' : '8px';
  },
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
  metadata: {
    currentWindow: { label: windowLabel },
    currentWebview: { label: windowLabel },
  },
  transformCallback(callback: (value: unknown) => void) {
    const id = ++callbackId;
    callbacks.set(id, callback);
    return id;
  },
  unregisterCallback(id: number) {
    callbacks.delete(id);
  },
  async invoke(command: string, args: any) {
    if (['qa_cancel_turn', 'qa_stop_recording', 'qa_submit_text'].includes(command)) {
      bridge.qaActions.push({ command, args });
    }
    if (command === 'set_capsule_interactive_regions') {
      bridge.inputRegions = args.rects;
      bridge.regionWrites.push(args.rects);
      return;
    }
    if (
      ['stop_dictation', 'cancel_dictation', 'qa_stop_recording', 'qa_cancel_turn'].includes(
        command,
      )
    ) {
      bridge.dictationActions.push(command);
      return;
    }
    if (command === 'plugin:event|listen') {
      const ids = listeners.get(args.event) ?? new Set<number>();
      ids.add(args.handler);
      listeners.set(args.event, ids);
      listenerTargets.set(args.handler, args.target ?? { kind: 'Any' });
      return args.handler;
    }
    if (command === 'plugin:event|unlisten') {
      listeners.get(args.event)?.delete(args.eventId);
      listenerTargets.delete(args.eventId);
      return;
    }
    if (command === 'get_startup_snapshot')
      return { contractVersion: '2.0.0', backend: { running: true } };
    if (command === 'get_settings') {
      const settings = {
        translationTargetLanguage: bridge.target.targetLanguage,
        capsuleStyle: 'classic',
        pipelineMode: bridge.pipelineMode,
        capsuleTranscriptEnabled: params.has('transcript'),
        ...(themeSync ? { themeMode: bridge.theme } : {}),
      };
      if (themeSync && ++bridge.themeReads === 1) {
        themeReadStarted();
        if (holdThemeRead) {
          await new Promise<void>((resolve) => {
            releaseThemeRead = resolve;
          });
        }
      }
      return settings;
    }
    if (command === 'plugin:app|set_app_theme') {
      bridge.nativeThemeWrites.push(args);
      return;
    }
    if (command === 'set_capsule_transcript_visible') {
      bridge.transcriptVisibility.push(args.visible);
      return;
    }
    if (command === 'plugin:app|set_app_theme') {
      bridge.nativeThemeWrites.push(args);
      return;
    }
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
    if (command === 'report_insert_fallback_card_height') {
      bridge.fallbackReports.push(args);
      return;
    }
    if (command === 'copy_text_to_clipboard') {
      bridge.copiedFallback.push(args.text);
      return;
    }
    if (command === 'qa_get_snapshot') return structuredClone(qaSnapshot);
    if (command === 'qa_window_set_expanded') {
      bridge.sizes.push(args.expanded);
      if (args.expanded && bridge.rejectQaExpansion) throw new Error('native resize failed');
      if (!bridge.holdQaGeometry) bridge.applyQaGeometry(args.expanded);
      return;
    }
    if (command === 'qa_window_dismiss') bridge.emit('chat-panel:closing', {});
    return null;
  },
};
Object.assign(window, { __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} } });
const { i18nReady } = await import('../../src/i18n');
await i18nReady;
const { InsertFallbackCard } = await import('../../src/features/capsule/InsertFallbackCard');
const { OpenLessPill } = await import('../../src/features/capsule/OpenLessCapsule');
const { CapsuleTranslationBadge, CapsuleTranscriptOverlay } =
  await import('../../src/features/capsule/LiveTranscriptPill');
const { QaPanel } = await import('../../src/features/qa/QaPanel');
const { Capsule } = await import('../../src/features/capsule/Capsule');
if (themeSync) {
  localStorage.setItem('ol.theme', 'light');
  const { initThemeMode } = await import('../../src/lib/themeMode');
  initThemeMode();
  initThemeMode();
  await firstThemeRead;
}
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
  params.has('fallback') ? (
    <AppViewport scale={false}>
      <InsertFallbackCard
        payload={{
          presentationId: 23,
          text: '待复制的长文本。\n'.repeat(36),
          reason: 'insertFailed',
        }}
      />
    </AppViewport>
  ) : params.has('rail') ? (
    <CapsuleTranscriptOverlay />
  ) : params.has('full') ? (
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
      <Capsule os={params.get('os') === 'mac' ? 'mac' : undefined} />
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
      <AppViewport scale={false}>
        <QaPanel />
      </AppViewport>
    </div>
  ) : (
    <AppViewport scale={false}>
      <CapsuleFixture />
    </AppViewport>
  ),
);
