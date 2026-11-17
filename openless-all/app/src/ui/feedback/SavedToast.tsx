import { useEffect, useId, useRef, type CSSProperties } from 'react';
import { useInteraction } from '../overlays/InteractionProvider';

export type SaveToastState = 'idle' | 'saving' | 'saved' | 'failed' | 'deleted';
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
  const { notify } = useInteraction(id);
  const action = useRef(onAction);
  action.current = onAction;
  const hasAction = Boolean(onAction);

  useEffect(() => {
    if (saveState === 'idle') return;
    const handle = notify({
      key: `saved-toast:${id}`,
      state: saveState,
      message,
      actionLabel,
      onAction: hasAction ? () => action.current?.() : undefined,
      durationMs,
    });
    return handle.dismiss;
  }, [id, notify, saveState, message, actionLabel, hasAction, durationMs, replayKey]);
  return null;
}
