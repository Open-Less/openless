import { useCallback, useEffect, useRef, useState } from 'react';
import {
  getRemoteInputStatus,
  listenRemoteInputErrors,
  listenRemoteInputStatusChanges,
  setRemoteLocale,
  type RemoteInputStartError,
  type RemoteInputStatus,
} from '../../api/input';
import { isTauriHost } from '../../runtime/environment';

export function useRemoteInputStatus(locale: string) {
  const [status, setStatus] = useState<RemoteInputStatus | null>(null);
  const [startError, setStartError] = useState<RemoteInputStartError | null>(null);
  const generation = useRef(0);

  const acceptStatus = useCallback(
    (next: (current: RemoteInputStatus | null) => RemoteInputStatus | null) => {
      generation.current += 1;
      setStatus(next);
    },
    [],
  );

  useEffect(() => {
    let active = true;
    const off: Array<() => void> = [];
    const refresh = async () => {
      const request = ++generation.current;
      try {
        const snapshot = await getRemoteInputStatus();
        if (active && generation.current === request) setStatus(snapshot);
      } catch {
        // An unreadable snapshot must not overwrite a live status or pairing token.
      }
    };
    const keep = (stop: () => void) => {
      if (active) off.push(stop);
      else stop();
    };
    void (async () => {
      if (isTauriHost()) {
        const listeners = await Promise.allSettled([
          listenRemoteInputStatusChanges(() => {
            if (!active) return;
            setStartError(null);
            void refresh();
          }).then(keep),
          listenRemoteInputErrors((error) => {
            if (!active) return;
            setStartError(error);
            void refresh();
          }).then(keep),
        ]);
        if (active) {
          for (const result of listeners) {
            if (result.status === 'rejected')
              console.warn('[remote-input] status listener setup failed', result.reason);
          }
        }
      }
      if (active) await refresh();
    })();
    return () => {
      active = false;
      off.forEach((stop) => stop());
    };
  }, []);

  useEffect(() => {
    void setRemoteLocale(locale).catch(() => {});
  }, [locale]);

  return { status, startError, acceptStatus };
}
