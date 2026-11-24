import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { setCapsuleLanguageMenu, type DictationTranslationTarget } from '../../api/dictation';
import { isTauriHost } from '../../runtime/environment';
import { waitForViewportGeometry } from '../../runtime/windows/viewportGeometry';
import { useAppViewport } from '../../ui/layout/AppViewport';

interface Reservation {
  sessionId: string;
  abort: AbortController;
  nativeWriteStarted: boolean;
  mounted: boolean;
}

export function useCapsuleLanguageMenu({
  anchor,
  target,
  visible,
  onFailure,
}: {
  anchor: RefObject<HTMLElement>;
  target: DictationTranslationTarget | null;
  visible: boolean;
  onFailure: () => void;
}) {
  const { boundary } = useAppViewport();
  const [open, setOpen] = useState(false);
  const reservation = useRef<Reservation>();
  const exiting = useRef<Reservation>();
  const tail = useRef<Promise<void>>(Promise.resolve());
  const active = useRef({ visible, target });
  active.current = { visible, target };

  const enqueue = useCallback((operation: () => Promise<void>) => {
    const result = tail.current.catch(() => undefined).then(operation);
    tail.current = result;
    return result;
  }, []);

  const release = useCallback(
    (owner: Reservation) => {
      if (exiting.current === owner) exiting.current = undefined;
      owner.abort.abort();
      void enqueue(async () => {
        if (owner.nativeWriteStarted) {
          owner.nativeWriteStarted = false;
          await setCapsuleLanguageMenu(false, owner.sessionId);
        }
      }).catch((error) =>
        console.warn('[capsule] failed to restore language menu geometry', error),
      );
    },
    [enqueue],
  );

  const close = useCallback(() => {
    const owner = reservation.current;
    if (!owner) return;
    reservation.current = undefined;
    owner.abort.abort();
    setOpen(false);
    if (owner.mounted) exiting.current = owner;
    else release(owner);
  }, [release]);

  const onOpenChange = async (next: boolean) => {
    if (!next) return close();
    const current = active.current;
    if (!current.visible || !current.target?.mutable || reservation.current) return;
    // The native owner is keyed by session, not by popup generation. Release a
    // prior exit before reserving again, even when both belong to one recording.
    if (exiting.current) release(exiting.current);
    const owner: Reservation = {
      sessionId: current.target.sessionId,
      abort: new AbortController(),
      nativeWriteStarted: false,
      mounted: false,
    };
    reservation.current = owner;
    try {
      if (isTauriHost()) {
        await enqueue(async () => {
          if (owner.abort.signal.aborted) return;
          owner.nativeWriteStarted = true;
          await setCapsuleLanguageMenu(true, owner.sessionId);
        });
        const viewport = boundary ?? document.documentElement;
        await waitForViewportGeometry(
          viewport,
          ({ width }) => {
            if (width < 100 || !anchor.current?.getClientRects().length) return false;
            const trigger = anchor.current.getBoundingClientRect();
            const frame = viewport.getBoundingClientRect();
            // Leave room for the menu inset and at least two complete options.
            const room = Math.max(trigger.top - frame.top, frame.bottom - trigger.bottom);
            return trigger.bottom <= frame.bottom + 1 && room >= 80;
          },
          owner.abort.signal,
        );
      }
      if (reservation.current !== owner || owner.abort.signal.aborted) return;
      if (
        !active.current.visible ||
        active.current.target?.sessionId !== owner.sessionId ||
        !active.current.target.mutable
      )
        return close();
      owner.mounted = true;
      setOpen(true);
    } catch {
      if (reservation.current === owner && !owner.abort.signal.aborted) {
        close();
        onFailure();
      }
    }
  };

  useEffect(() => {
    if (
      reservation.current &&
      (!visible || !target?.mutable || target.sessionId !== reservation.current.sessionId)
    )
      close();
  }, [close, visible, target?.mutable, target?.sessionId]);

  useEffect(() => {
    const onKeydown = (event: KeyboardEvent) => {
      if (
        event.key === 'Escape' &&
        !event.isComposing &&
        reservation.current &&
        !reservation.current.mounted
      ) {
        event.preventDefault();
        close();
      }
    };
    window.addEventListener('blur', close);
    window.addEventListener('keydown', onKeydown);
    return () => {
      window.removeEventListener('blur', close);
      window.removeEventListener('keydown', onKeydown);
      if (reservation.current) release(reservation.current);
      if (exiting.current) release(exiting.current);
      reservation.current = undefined;
    };
  }, [close, release]);

  return {
    open,
    close,
    sessionForChoice: () => reservation.current?.sessionId ?? exiting.current?.sessionId,
    onOpenChange,
    onOpenChangeComplete(next: boolean) {
      if (!next && exiting.current) release(exiting.current);
    },
  };
}
