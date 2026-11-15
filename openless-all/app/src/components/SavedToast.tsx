import { useEffect, useId, useRef, type CSSProperties } from 'react';
import { dismissNotification, notify } from '../lib/notifications';
export type SaveToastState = 'idle' | 'saving' | 'saved' | 'deleted' | 'failed';
export type ToastSlideFrom = 'right' | 'top';
interface SavedToastProps {
  saveState: SaveToastState;
  message: string;
  offsetStyle?: Pick<CSSProperties, 'top' | 'right' | 'left' | 'bottom' | 'position'>;
  slideFrom?: ToastSlideFrom;
  actionLabel?: string;
  onAction?: () => void;
  durationMs?: number;
  replayKey?: number;
}
export function SavedToast({
  saveState,
  message,
  actionLabel,
  onAction,
  durationMs,
  replayKey,
}: SavedToastProps) {
  const id = useId();
  const action = useRef(onAction);
  action.current = onAction;
  const hasAction = Boolean(onAction);
  const previousState = useRef<SaveToastState>('idle');
  useEffect(() => {
    if (saveState === 'idle') {
      if (previousState.current !== 'idle') dismissNotification(id);
    } else {
      notify({
        id,
        state: saveState,
        message,
        actionLabel,
        onAction: hasAction ? () => action.current?.() : undefined,
        durationMs,
      });
    }
    previousState.current = saveState;
  }, [id, saveState, message, actionLabel, hasAction, durationMs, replayKey]);
  useEffect(() => () => dismissNotification(id), [id]);
  return null;
}
