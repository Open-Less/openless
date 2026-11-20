import { useCallback, useEffect, useRef } from 'react';
import { useToast } from '../src/ui/overlays/InteractionProvider';

type Severity = 'warning' | 'error';

export function useAskNotice() {
  const toast = useToast();
  return useCallback(
    (message: string, severity: Severity, key?: string) =>
      toast[severity](message, {
        key,
        durationMs: severity === 'warning' ? 6000 : 0,
      }),
    [toast],
  );
}

export function AskNotice({ message, severity }: { message?: string; severity: Severity }) {
  const notice = useAskNotice();
  const last = useRef<{ message?: string; severity?: Severity }>({});
  useEffect(() => {
    if (!message || (message === last.current.message && severity === last.current.severity))
      return;
    last.current = { message, severity };
    notice(message, severity, 'ask-notice');
  }, [message, severity, notice]);
  return null;
}
