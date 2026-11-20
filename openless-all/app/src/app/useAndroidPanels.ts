import { useEffect, useRef, useState } from 'react';
import {
  cancelVoiceEditSession,
  getVoiceEditState,
  listenEmbeddedVoiceEditDismissed,
  listenEmbeddedVoiceEditShown,
} from '../api/voice-edit';
import { useAppViewport } from '../ui/layout/AppViewport';
import { createAndroidPanelState, type AndroidPanelView } from './androidPanelState';

const HISTORY_KEY = 'openlessEmbeddedPanel';

export function useAndroidPanels(enabled: boolean) {
  const { modalStack } = useAppViewport();
  const [panel, setPanel] = useState<AndroidPanelView>({ kind: null, epoch: 0 });
  const controller = useRef<ReturnType<typeof createAndroidPanelState>>();

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let releasingHistory = false;
    const unlisten: Array<() => void> = [];
    const history = window.history;
    const hasEntry = () => history.state?.[HISTORY_KEY] === true;
    const armBack = () => {
      if (!hasEntry() && !releasingHistory)
        history.pushState({ ...history.state, [HISTORY_KEY]: true }, '', location.href);
    };
    const releaseBack = () => {
      if (!hasEntry() || releasingHistory) return;
      releasingHistory = true;
      history.back();
    };

    const route = createAndroidPanelState(
      async (_kind, isCurrent) => {
        const session = await getVoiceEditState();
        if (isCurrent()) await cancelVoiceEditSession(session?.sessionId);
      },
      (next) => {
        setPanel(next);
        if (next.kind) armBack();
        else releaseBack();
      },
    );
    controller.current = route;

    const onBack = () => {
      const current = route.current();
      if (releasingHistory) {
        releasingHistory = false;
        if (current.kind) armBack();
      } else if (current.kind && !hasEntry()) {
        // Preserve a close affordance even if IPC rejects or a new panel wins.
        armBack();
        void route.close(current);
      }
    };
    const onKey = (event: KeyboardEvent) => {
      const current = route.current();
      if (
        !current.kind ||
        event.defaultPrevented ||
        event.isComposing ||
        event.keyCode === 229 ||
        event.key !== 'Escape' ||
        modalStack.getActive()
      )
        return;
      event.preventDefault();
      void route.close(current);
    };
    window.addEventListener('popstate', onBack);
    window.addEventListener('keydown', onKey);

    function subscribe(listen: (onEvent: () => void) => Promise<() => void>, handler: () => void) {
      return listen(() => {
        if (!disposed) handler();
      }).then((stop) => {
        if (disposed) stop();
        else unlisten.push(stop);
      });
    }

    void (async () => {
      const observed = route.generation();
      await Promise.all([
        subscribe(listenEmbeddedVoiceEditShown, () => {
          void route.switchTo('voice-edit');
        }),
        subscribe(listenEmbeddedVoiceEditDismissed, () => route.nativeDismiss('voice-edit')),
      ]);
      if (disposed || observed !== route.generation()) return;
      // Recreated WebViews may have missed their initial show event.
      const [voice] = await Promise.allSettled([getVoiceEditState()]);
      if (disposed || observed !== route.generation()) return;
      const phase = voice.status === 'fulfilled' ? voice.value?.phase : undefined;
      if (phase && phase !== 'completed' && phase !== 'cancelled') {
        await route.switchTo('voice-edit');
      } else {
        releaseBack();
      }
    })().catch((error: unknown) => {
      if (!disposed) console.warn('[android-panels] route listener setup failed', error);
    });

    return () => {
      disposed = true;
      route.dispose();
      window.removeEventListener('popstate', onBack);
      window.removeEventListener('keydown', onKey);
      unlisten.forEach((stop) => stop());
      if (controller.current === route) controller.current = undefined;
    };
  }, [enabled, modalStack]);

  return {
    panel,
    close: (expected: AndroidPanelView) =>
      controller.current?.close(expected) ?? Promise.resolve(false),
    didClose(expected: AndroidPanelView) {
      const route = controller.current;
      if (route && route.current().epoch === expected.epoch && expected.kind)
        route.nativeDismiss(expected.kind);
    },
  };
}
