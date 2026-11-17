import { HARMONY_CHANGE_EVENT, listenTauriEvent, requireBackendReady } from '../runtime/transport';
import { isHarmonyRuntime } from '../runtime/environment';

export async function listenVocabularyChanges(onChange: () => void): Promise<() => void> {
  if (!isHarmonyRuntime()) return listenTauriEvent('vocab:updated', onChange);
  await requireBackendReady();
  window.addEventListener(HARMONY_CHANGE_EVENT, onChange);
  return () => window.removeEventListener(HARMONY_CHANGE_EVENT, onChange);
}
