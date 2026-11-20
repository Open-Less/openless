import { useEffect, useRef } from 'react';
import { useInteraction } from '../src/ui/overlays/InteractionProvider';

export function useAskErrors() {
  const { warn } = useInteraction();
  return warn;
}

export function AskErrorDialog({
  message,
  title = 'Ask Anything',
}: {
  message?: string;
  title?: string;
}) {
  const warn = useAskErrors();
  const last = useRef<string>();
  useEffect(() => {
    if (!message || message === last.current) return;
    last.current = message;
    void warn({ title, description: message, dismissLabel: 'OK' });
  }, [message, title, warn]);
  return null;
}
