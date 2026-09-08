import { createContext, useCallback, useContext, useEffect, useId, useMemo, useRef, useState } from 'react';

export const ProviderLeaveContext = createContext<((id: string, flush: () => Promise<boolean>) => () => void) | null>(null);

/** 同一表单的写入在关闭/切换前收敛；版本只含计数，不携带凭据内容。 */
export function useProviderForm() {
  const parent = useContext(ProviderLeaveContext);
  const id = useId();
  const version = useRef(0);
  const [revision, setRevision] = useState(0);
  const [blocked, setBlocked] = useState<Record<string, boolean>>({});
  const blockers = useRef<Record<string, boolean>>({});
  const flushers = useRef(new Map<string, () => Promise<boolean>>());
  const busy = useRef(false);
  const [leaving, setLeaving] = useState(false);
  const invalidate = useCallback((account = '') => {
    if (!['ark.model_id', 'asr.model', 'omni.model'].includes(account)) version.current += 1;
    setRevision(value => value + 1);
  }, []);
  const track = useCallback((account: string, next: boolean) => {
    blockers.current[account] = next;
    setBlocked(previous => previous[account] === next ? previous : { ...previous, [account]: next });
  }, []);
  const register = useCallback((account: string, flush: () => Promise<boolean>) => {
    flushers.current.set(account, flush);
    return () => { flushers.current.delete(account); delete blockers.current[account];
      setBlocked(previous => { const next = { ...previous }; delete next[account]; return next; }); };
  }, []);
  const finish = useCallback(async (action: () => void | Promise<void>) => {
    if (busy.current) return false;
    busy.current = true;
    setLeaving(true);
    invalidate();
    try {
      const results = await Promise.all([...flushers.current.values()].map(flush => flush()));
      if (!results.every(Boolean) || Object.entries(blockers.current).some(([account, blocked]) => blocked && !flushers.current.has(account))) return false;
      await action();
      return true;
    } finally { busy.current = false; setLeaving(false); }
  }, [invalidate]);
  useEffect(() => parent?.(id, () => finish(() => undefined)), [parent, id, finish]);
  return useMemo(() => ({ version, revision, blocked, leaving, invalidate, track, register, finish }),
    [revision, blocked, leaving, invalidate, track, register, finish]);
}

export const ProviderFormContext = createContext<ReturnType<typeof useProviderForm> | null>(null);
