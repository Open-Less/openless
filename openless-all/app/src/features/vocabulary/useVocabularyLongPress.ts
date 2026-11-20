import { useEffect, useRef, type MouseEvent, type PointerEvent } from 'react';

const HOLD_MS = 500;
const MOVE_THRESHOLD = 8;

export function useVocabularyLongPress(
  disabled: boolean,
  onHold: () => void,
  ignore?: (target: EventTarget) => boolean,
) {
  const pending = useRef<{ id: number; x: number; y: number; timer: number } | null>(null);
  const consumed = useRef(false);

  function cancel() {
    if (pending.current) window.clearTimeout(pending.current.timer);
    pending.current = null;
  }

  useEffect(() => {
    if (disabled) cancel();
    return cancel;
  }, [disabled]);

  return {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      cancel();
      if (!event.isPrimary || event.button !== 0) return;
      consumed.current = false;
      if (disabled || ignore?.(event.target)) return;

      pending.current = {
        id: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        timer: window.setTimeout(() => {
          pending.current = null;
          consumed.current = true;
          onHold();
        }, HOLD_MS),
      };
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      const press = pending.current;
      if (
        press?.id === event.pointerId &&
        Math.hypot(event.clientX - press.x, event.clientY - press.y) > MOVE_THRESHOLD
      )
        cancel();
    },
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onPointerLeave: cancel,
    onClickCapture(event: MouseEvent<HTMLElement>) {
      // Releasing a hold must not also activate the regular click action.
      if (!consumed.current || event.detail === 0) return;
      event.preventDefault();
      event.stopPropagation();
      consumed.current = false;
    },
  };
}
