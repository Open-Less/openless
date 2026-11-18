import { useEffect, useId, useRef, type CSSProperties } from 'react';
import type { NotificationHandle } from '../../api/interaction';
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
  const handle = useRef<NotificationHandle>();
  const action = useRef(onAction);
  action.current = onAction;
  const hasAction = Boolean(onAction);

  useEffect(() => {
    if (saveState === 'idle') {
      handle.current?.dismiss();
      handle.current = undefined;
      return;
    }
    const options = {
      key: `saved-toast:${id}`,
      state: saveState,
      message,
      actionLabel,
      onAction: hasAction ? () => action.current?.() : undefined,
      durationMs,
    };
    handle.current = notify(options);
  }, [id, notify, saveState, message, actionLabel, hasAction, durationMs, replayKey]);
  useEffect(
    () => () => {
      handle.current?.dismiss();
      handle.current = undefined;
    },
    [notify],
  );
  return null;
}
