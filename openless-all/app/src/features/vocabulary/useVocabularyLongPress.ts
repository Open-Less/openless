import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MouseEvent,
  type PointerEvent,
} from 'react';

const HOLD_MS = 500;
const MOVE_THRESHOLD = 8;

export function useVocabularyLongPress(
  disabled: boolean,
  onHold: () => void,
  ignore?: (target: EventTarget) => boolean,
) {
  const pending = useRef<{ id: number; x: number; y: number; timer: number } | null>(null);
  const consumed = useRef(false);
  const [pressing, setPressing] = useState(false);

  const clearTimer = useCallback(() => {
    if (!pending.current) return;
    window.clearTimeout(pending.current.timer);
    pending.current = null;
  }, []);

  const cancel = useCallback(() => {
    clearTimer();
    setPressing(false);
  }, [clearTimer]);

  useEffect(() => {
    if (disabled) cancel();
    return clearTimer;
  }, [cancel, clearTimer, disabled]);

  useEffect(() => {
    if (!pressing) return;
    window.addEventListener('blur', cancel);
    return () => window.removeEventListener('blur', cancel);
  }, [cancel, pressing]);

  const handlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      cancel();
      if (!event.isPrimary || event.button !== 0) return;
      consumed.current = false;
      if (disabled || ignore?.(event.target)) return;

      setPressing(true);
      pending.current = {
        id: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        timer: window.setTimeout(() => {
          pending.current = null;
          setPressing(false);
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

  return { pressing, handlers };
}
