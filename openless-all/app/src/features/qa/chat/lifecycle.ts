import { useEffect, useState } from 'react';
import { isTauriHost } from '../../../runtime/environment';
import { listenTauriEvent } from '../../../runtime/transport';

export function useChatPanelLifecycle(nativeWindow: boolean): {
  enterEpoch: number;
  closing: boolean;
} {
  const [lifecycle, setLifecycle] = useState({ enterEpoch: 0, closing: false });

  useEffect(() => {
    if (!nativeWindow || !isTauriHost()) return;
    const handles: (() => void)[] = [];
    let stopped = false;
    function attach<T>(event: string, change: () => void) {
      return listenTauriEvent<T>(event, () => {
        if (!stopped) change();
      }).then((remove) => {
        if (stopped) remove();
        else handles.push(remove);
      });
    }
    void Promise.all([
      attach('chat-panel:shown', () => {
        setLifecycle(({ enterEpoch }) => ({ enterEpoch: enterEpoch + 1, closing: false }));
      }),
      attach('chat-panel:closing', () => {
        setLifecycle(({ enterEpoch }) => ({ enterEpoch, closing: true }));
      }),
    ]).catch((error) => {
      if (!stopped) console.error('[chat-panel] lifecycle listener setup failed', error);
    });
    return () => {
      stopped = true;
      handles.forEach((remove) => remove());
    };
  }, [nativeWindow]);

  return lifecycle;
}
