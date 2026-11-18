import { useCallback, useEffect, useRef, useState } from 'react';
import { getRemoteInputStatus, setRemoteLocale, type RemoteInputStatus } from '../../api/input';
import { isTauriHost } from '../../runtime/environment';
import { listenTauriEvent } from '../../runtime/transport';

export function useRemoteInputStatus(locale: string) {
  const [status, setStatus] = useState<RemoteInputStatus | null>(null);
  const [startError, setStartError] = useState<{ reason: string; port: number } | null>(null);
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
    async function subscribe<T>(name: string, handler: (payload: T) => void) {
      const stop = await listenTauriEvent<T>(name, (value) => {
        if (active) handler(value);
      });
      if (active) off.push(stop);
      else stop();
    }
    void (async () => {
      if (isTauriHost()) {
        await Promise.allSettled([
          subscribe('remote-input:running', () => {
            setStartError(null);
            void refresh();
          }),
          subscribe<{ reason?: string; port?: number } | null>('remote-input:error', (payload) => {
            setStartError({ reason: payload?.reason ?? '', port: payload?.port ?? 0 });
            void refresh();
          }),
        ]);
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
