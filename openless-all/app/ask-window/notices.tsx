import { useCallback, useEffect, useRef } from 'react';
import { useInteraction } from '../src/ui/overlays/InteractionProvider';

type Severity = 'warning' | 'error';

export function useAskNotice() {
  const { notify } = useInteraction();
  return useCallback(
    (message: string, severity: Severity, key?: string) =>
      notify({
        message,
        severity,
        key,
        ...(severity === 'warning' ? { durationMs: 6000 } : { state: 'failed' }),
      }),
    [notify],
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
