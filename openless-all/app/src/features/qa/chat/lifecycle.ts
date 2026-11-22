import { useEffect, useRef, useState } from 'react';
import { listenQaWindowClosing, listenQaWindowShown } from '../../../api/qa';
import { isTauriHost } from '../../../runtime/environment';

export function useChatPanelLifecycle(
  nativeWindow: boolean,
  onClosingChange: (closing: boolean) => void,
) {
  const onClosingChangeRef = useRef(onClosingChange);
  onClosingChangeRef.current = onClosingChange;
  const [lifecycle, setLifecycle] = useState({ enterEpoch: 0, closing: false });

  useEffect(() => {
    if (!nativeWindow || !isTauriHost()) return;
    const handles: (() => void)[] = [];
    let stopped = false;
    function attach(listen: (onEvent: () => void) => Promise<() => void>, change: () => void) {
      return listen(() => {
        if (!stopped) change();
      }).then((remove) => {
        if (stopped) remove();
        else handles.push(remove);
      });
    }
    void Promise.all([
      attach(listenQaWindowShown, () => {
        onClosingChangeRef.current(false);
        setLifecycle(({ enterEpoch }) => ({ enterEpoch: enterEpoch + 1, closing: false }));
      }),
      attach(listenQaWindowClosing, () => {
        onClosingChangeRef.current(true);
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
