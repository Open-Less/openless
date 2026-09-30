// lifecycle.ts — appear/disappear animation signals for the chat panels (qa /
// less-computer).
//
// The floating window is a persistent webview (reused across hide/show, never
// remounted), so the CSS enter animation plays only on first mount — every later
// wake-up would pop in abruptly. The backend emits lifecycle events on the show/hide
// paths:
//   · `chat-panel:shown`   → enterEpoch+1 (used as a key to replay olchat-shell-in)
//   · `chat-panel:closing` → closing=true (adds olchat-shell-out for the exit; the
//     backend actually hides 240ms later; a shown during that window resets it)
// Browser preview (non-Tauri) has no such events; keep the first-mount enter animation.

import { useEffect, useState } from 'react';
import { isTauri } from '../../lib/ipc/shared';

export function useChatPanelLifecycle(): { enterEpoch: number; closing: boolean } {
  const [enterEpoch, setEnterEpoch] = useState(0);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    let unShown: (() => void) | undefined;
    let unClosing: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const shownHandle = await listen('chat-panel:shown', () => {
          setClosing(false);
          setEnterEpoch((epoch) => epoch + 1);
        });
        const closingHandle = await listen('chat-panel:closing', () => {
          setClosing(true);
        });
        if (cancelled) {
          shownHandle();
          closingHandle();
        } else {
          unShown = shownHandle;
          unClosing = closingHandle;
        }
      } catch (error) {
        console.error('[chat-panel] lifecycle listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unShown?.();
      unClosing?.();
    };
  }, []);

  return { enterEpoch, closing };
}
