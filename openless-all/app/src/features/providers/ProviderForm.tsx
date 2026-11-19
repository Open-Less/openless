import {
  createContext,
  useCallback,
  useContext,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

export const ProviderLeaveContext = createContext<
  ((id: string, flush: () => Promise<boolean>) => () => void) | null
>(null);

/** Writes for the same form are flushed before close/switch; the version is a counter only and
 *  never carries credential content. */
export function useProviderForm() {
  const parent = useContext(ProviderLeaveContext);
  const id = useId();
  const version = useRef(0);
  const [revision, setRevision] = useState(0);
  const [blocked, setBlocked] = useState<Record<string, boolean>>({});
  const blockers = useRef<Record<string, boolean>>({});
  const flushers = useRef(new Map<string, () => Promise<boolean>>());
  const busy = useRef(false);
  const lifetime = useRef<symbol | null>(null);
  useLayoutEffect(() => {
    lifetime.current = Symbol();
    return () => {
      lifetime.current = null;
    };
  }, []);
  const [leaving, setLeaving] = useState(false);
  const invalidate = useCallback((account = '') => {
    if (!['ark.model_id', 'asr.model', 'omni.model'].includes(account)) version.current += 1;
    setRevision((value) => value + 1);
  }, []);
  const track = useCallback((account: string, next: boolean) => {
    blockers.current[account] = next;
    setBlocked((previous) =>
      previous[account] === next ? previous : { ...previous, [account]: next },
    );
  }, []);
  const register = useCallback((account: string, flush: () => Promise<boolean>) => {
    flushers.current.set(account, flush);
    return () => {
      if (flushers.current.get(account) !== flush) return;
      flushers.current.delete(account);
      delete blockers.current[account];
      setBlocked((previous) => {
        const next = { ...previous };
        delete next[account];
        return next;
      });
    };
  }, []);
  const finish = useCallback(
    async (action: () => void | boolean | Promise<void | boolean>) => {
      const owner = lifetime.current;
      if (busy.current || owner === null) return false;
      busy.current = true;
      setLeaving(true);
      invalidate();
      try {
        const fields = [...flushers.current];
        // Keep the form locked until every pending field settles, even if one rejects.
        const results = await Promise.allSettled(fields.map(async ([, flush]) => flush()));
        if (
          lifetime.current !== owner ||
          results.some((result) => result.status === 'rejected' || !result.value) ||
          fields.length !== flushers.current.size ||
          fields.some(([account, flush]) => flushers.current.get(account) !== flush) ||
          Object.entries(blockers.current).some(
            ([account, blocked]) => blocked && !flushers.current.has(account),
          )
        )
          return false;
        return (await action()) !== false;
      } finally {
        busy.current = false;
        if (lifetime.current === owner) setLeaving(false);
      }
    },
    [invalidate],
  );
  useLayoutEffect(() => parent?.(id, () => finish(() => undefined)), [parent, id, finish]);
  return useMemo(
    () => ({ version, revision, blocked, leaving, invalidate, track, register, finish }),
    [revision, blocked, leaving, invalidate, track, register, finish],
  );
}

export const ProviderFormContext = createContext<ReturnType<typeof useProviderForm> | null>(null);
