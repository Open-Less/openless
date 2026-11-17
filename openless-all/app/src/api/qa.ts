export * from '../lib/ipc/qa';
export { chatPanelFocusKeyboard } from '../lib/ipc/chat-panel';

import { isTauriHost } from '../runtime/environment';
import { invokeOrPreview, requireBackendReady } from '../runtime/transport';

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
