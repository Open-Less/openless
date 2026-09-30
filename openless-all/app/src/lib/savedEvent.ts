// savedEvent.ts — cross-component unified "saved / failed" event channel.
//
// Emit: any component calls emitSaved(...) when a save succeeds / fails.
// Listen: root containers (Settings / Translation / SelectionAsk) subscribe via
// useSavedToastListener, feeding state to <SavedToast>, a pill floating top-right.
//
// A DOM CustomEvent (instead of React Context) lets deep leaf components like CredentialField /
// ProviderTools avoid threading a dispatcher down the props chain, same convention as
// NAVIGATE_LOCAL_ASR_EVENT.

import { useEffect, useState } from 'react';

export const SAVED_TOAST_EVENT = 'openless:saved-toast';

export type SavedToastEventState = 'saving' | 'saved' | 'failed';

export interface SavedToastDetail {
  state: SavedToastEventState;
  message: string;
}

export function emitSaved(state: SavedToastEventState, message: string): void {
  window.dispatchEvent(
    new CustomEvent<SavedToastDetail>(SAVED_TOAST_EVENT, { detail: { state, message } }),
  );
}

interface ToastSnapshot {
  state: 'idle' | SavedToastEventState;
  message: string;
}

const IDLE_SNAPSHOT: ToastSnapshot = { state: 'idle', message: '' };

/**
 * Subscribe to saved-toast events, auto-managing the "return to idle 1.6s after a non-saving
 * state" logic. A saving state stays visible until the next event overwrites it (so saving
 * does not disappear mid-way through a long task).
 */
export function useSavedToastListener(): ToastSnapshot {
  const [snapshot, setSnapshot] = useState<ToastSnapshot>(IDLE_SNAPSHOT);
  useEffect(() => {
    let timer: number | null = null;
    const handle = (event: Event) => {
      const detail = (event as CustomEvent<SavedToastDetail>).detail;
      if (!detail) return;
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
      setSnapshot({ state: detail.state, message: detail.message });
      if (detail.state !== 'saving') {
        timer = window.setTimeout(() => {
          setSnapshot(IDLE_SNAPSHOT);
          timer = null;
        }, 1600);
      }
    };
    window.addEventListener(SAVED_TOAST_EVENT, handle);
    return () => {
      window.removeEventListener(SAVED_TOAST_EVENT, handle);
      if (timer !== null) window.clearTimeout(timer);
    };
  }, []);
  return snapshot;
}
