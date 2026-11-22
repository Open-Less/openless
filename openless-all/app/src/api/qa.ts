export * from '../lib/ipc/qa';
export { chatPanelFocusKeyboard } from '../lib/ipc/chat-panel';

import type { QaStatePayload } from './contracts';
import { isTauriHost } from '../runtime/environment';
import { invokeOrPreview, listenTauriEvent, requireBackendReady } from '../runtime/transport';

export interface QaRecordingLevel {
  sessionId: string;
  level: number;
}

export function listenQaStateChanges(
  onState: (state: QaStatePayload) => void,
): Promise<() => void> {
  return listenTauriEvent<QaStatePayload>('qa:state', onState);
}

export function listenQaLevelChanges(
  onLevel: (recording: QaRecordingLevel) => void,
): Promise<() => void> {
  return listenTauriEvent<QaRecordingLevel>('qa:level', onLevel);
}

export function listenQaDismissed(onDismissed: () => void): Promise<() => void> {
  return listenTauriEvent('qa:dismiss', onDismissed);
}

export function listenQaWindowShown(onShown: () => void): Promise<() => void> {
  return listenTauriEvent('chat-panel:shown', onShown);
}

export function listenQaWindowClosing(onClosing: () => void): Promise<() => void> {
  return listenTauriEvent('chat-panel:closing', onClosing);
}

export function listenEmbeddedQaShown(onShown: () => void): Promise<() => void> {
  return listenQaStateChanges((state) => {
    // ShowQa sends a bare idle signal. The idle dismissal snapshot carries messages.
    if (state.kind === 'idle' && state.sessionId == null && state.messages === undefined) onShown();
  });
}

export async function startQaReadAloud(
  text: string,
  requestId: string,
  onAudio: (bytes: number[]) => void,
): Promise<void> {
  if (!isTauriHost()) throw new Error('Read aloud requires a Tauri QA window');
  await requireBackendReady();
  const { Channel, invoke } = await import('@tauri-apps/api/core');
  const audio = new Channel<number[]>();
  audio.onmessage = onAudio;
  await invoke('deepseek_tts_start', { text, requestId, audio });
}

export async function stopQaReadAloud(requestId: string): Promise<void> {
  if (!isTauriHost()) return;
  await invokeOrPreview('deepseek_tts_stop', { requestId }, () => undefined);
}
