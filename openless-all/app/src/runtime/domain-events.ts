import type { BackendEvent } from '../api/contracts';
import { listenPreviewChange } from '../preview/events';
import { getRuntimeEnvironment } from './environment';
import { HARMONY_CHANGE_EVENT, listenTauriEvent, requireBackendReady } from './transport';

async function listenHarmonyChanges(onChange: () => void): Promise<() => void> {
  await requireBackendReady();
  window.addEventListener(HARMONY_CHANGE_EVENT, onChange);
  return () => window.removeEventListener(HARMONY_CHANGE_EVENT, onChange);
}

export function listenHistoryChanges(onChange: (revision?: number) => void): Promise<() => void> {
  switch (getRuntimeEnvironment().host) {
    case 'preview':
      return Promise.resolve(listenPreviewChange('history', onChange));
    case 'harmony':
      return listenHarmonyChanges(onChange);
    case 'tauri':
      return listenTauriEvent<BackendEvent>('backend:event', (event) => {
        if (event.kind.type === 'history_changed') {
          const revision = (event.kind.payload as { revision?: unknown } | undefined)?.revision;
          onChange(
            typeof revision === 'number' && Number.isSafeInteger(revision) && revision >= 0
              ? revision
              : undefined,
          );
        } else if (event.kind.type === 'cloud_sync_restore_completed') {
          onChange();
        }
      });
  }
}

export function listenPreferenceChanges(onChange: () => void): Promise<() => void> {
  switch (getRuntimeEnvironment().host) {
    case 'preview':
      return Promise.resolve(listenPreviewChange('preferences', onChange));
    case 'harmony':
      return listenHarmonyChanges(onChange);
    case 'tauri':
      return listenTauriEvent('prefs:changed', onChange);
  }
}

export function listenStylePackChanges(onChange: () => void): Promise<() => void> {
  switch (getRuntimeEnvironment().host) {
    case 'preview':
      return Promise.resolve(listenPreviewChange('style-packs', onChange));
    case 'harmony':
      return listenHarmonyChanges(onChange);
    case 'tauri':
      return listenTauriEvent<BackendEvent>('backend:event', (event) => {
        if (event.kind.type === 'style_packs_changed') onChange();
      });
  }
}
