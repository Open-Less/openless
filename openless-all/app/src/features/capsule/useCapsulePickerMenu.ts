import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { isTauriHost } from '../../runtime/environment';
import { waitForViewportGeometry } from '../../runtime/windows/viewportGeometry';
import { useAppViewport } from '../../ui/layout/AppViewport';
import type {
  CapsulePickerDeps,
  CapsulePickerKind,
  CapsulePickerMenu,
  CapsulePickerTarget,
} from './capsulePickerMenu';

export function useCapsulePickerMenu({
  controller,
  kind,
  anchor,
  target,
  visible,
  setNativeMenu,
  onFailure,
  onStaleStyleRequest,
}: {
  controller: CapsulePickerMenu;
  kind: CapsulePickerKind;
  anchor: RefObject<HTMLElement>;
  target: CapsulePickerTarget | null;
  visible: boolean;
  setNativeMenu: CapsulePickerDeps['setNativeMenu'];
  onFailure: () => void;
  onStaleStyleRequest?: CapsulePickerDeps['onStaleStyleRequest'];
}) {
  const { boundary } = useAppViewport();
  const [open, setOpen] = useState(false);
  const config = useRef({ visible, target, boundary, onFailure, onStaleStyleRequest });
  config.current = { visible, target, boundary, onFailure, onStaleStyleRequest };

  const close = useCallback(() => controller.close(kind), [controller, kind]);
  const openMenu = useCallback(
    (requestId?: string) => {
      const current = config.current;
      if (!current.visible || !current.target?.mutable) return;
      return controller.open(
        { kind, sessionId: current.target.sessionId, requestId },
        {
          native: isTauriHost(),
          setNativeMenu,
          onOpenChange: setOpen,
          onFailure: () => config.current.onFailure(),
          onStaleStyleRequest: (request) => config.current.onStaleStyleRequest?.(request),
          waitForGeometry: async (signal) => {
            const viewport = current.boundary ?? document.documentElement;
            await waitForViewportGeometry(
              viewport,
              ({ width }) => {
                if (width < 100 || !anchor.current?.getClientRects().length) return false;
                const trigger = anchor.current.getBoundingClientRect();
                const frame = viewport.getBoundingClientRect();
                const room = Math.max(trigger.top - frame.top, frame.bottom - trigger.bottom);
                return trigger.bottom <= frame.bottom + 1 && room >= 80;
              },
              signal,
            );
          },
        },
      );
    },
    [anchor, controller, kind, setNativeMenu],
  );

  useLayoutEffect(() => {
    controller.update(kind, visible, target);
  }, [controller, kind, visible, target]);

  useEffect(() => {
    const onKeydown = (event: KeyboardEvent) => controller.handleKeydown(kind, event);
    window.addEventListener('blur', close);
    if (kind === 'language') window.addEventListener('keydown', onKeydown);
    return () => {
      window.removeEventListener('blur', close);
      window.removeEventListener('keydown', onKeydown);
      controller.dispose(kind);
    };
  }, [close, controller, kind]);

  return {
    open,
    openMenu,
    close,
    sessionForChoice: () => controller.sessionForChoice(kind),
    owns: (sessionId: string, requestId: string) => controller.owns(kind, sessionId, requestId),
    onOpenChange(next: boolean) {
      if (next) void openMenu();
      else close();
    },
    onOpenChangeComplete(next: boolean) {
      controller.openChangeComplete(kind, next);
    },
  };
}
