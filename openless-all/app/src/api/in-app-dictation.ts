import { isHarmonyRuntime } from '../runtime/environment';
import { invokeOrPreview } from '../runtime/transport';

export interface VoiceState {
  phase: 'idle' | 'starting' | 'recording' | 'processing' | 'cancelling';
  level: number;
  error: string;
}

export interface VoiceResult {
  rawText: string;
  polishedText: string;
}

const phases = new Set<VoiceState['phase']>([
  'idle',
  'starting',
  'recording',
  'processing',
  'cancelling',
]);

export function listenInAppDictation(onChange: (state: VoiceState) => void): () => void {
  if (!isHarmonyRuntime()) return () => {};
  const receive = (event: Event) => {
    const next = (event as CustomEvent<VoiceState>).detail;
    if (next && phases.has(next.phase)) onChange(next);
  };
  window.addEventListener('openless:harmony-dictation', receive);
  return () => window.removeEventListener('openless:harmony-dictation', receive);
}

export function finishInAppDictation(): Promise<VoiceResult> {
  return invokeOrPreview('stop_dictation', undefined, () => {
    throw new Error('In-app dictation is unavailable in browser preview');
  });
}
