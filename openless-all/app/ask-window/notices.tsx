import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useInteraction } from '../src/ui/overlays/InteractionProvider';
import { askErrorCode, askErrorLabel } from './client';

type Severity = 'warning' | 'error';
type Notice = { id: number; code: string; message: string; severity: Severity };
type Report = (error: unknown, severity: Severity, operation?: string) => { dismiss(): void };
const AskNoticeContext = createContext<{
  notice?: Notice;
  report: Report;
  clear(): void;
}>({ report: () => ({ dismiss() {} }), clear() {} });

export function AskNoticeProvider({ children }: { children: ReactNode }) {
  const [notice, setNotice] = useState<Notice>();
  const next = useRef(0);
  const clear = useCallback(() => setNotice(undefined), []);
  const report = useCallback<Report>((error, severity, operation) => {
    const id = ++next.current;
    const code = askErrorCode(error, operation);
    setNotice({ id, code, message: askErrorLabel(code), severity });
    return { dismiss: () => setNotice((current) => (current?.id === id ? undefined : current)) };
  }, []);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(clear, 4500);
    return () => window.clearTimeout(timer);
  }, [notice, clear]);
  return <AskNoticeContext.Provider value={{ notice, report, clear }}>{children}</AskNoticeContext.Provider>;
}

export function useAskNotice() {
  const { report } = useContext(AskNoticeContext);
  const mounted = useRef(false);
  useLayoutEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  return useCallback<Report>((...args) =>
    mounted.current ? report(...args) : { dismiss() {} }, [report]);
}

export function useClearAskNotice() {
  return useContext(AskNoticeContext).clear;
}

export function useCurrentAskNotice() {
  return useContext(AskNoticeContext).notice;
}

// The shared notification host is outside the composer and never affects its geometry.
export function AskNoticePopup({ expanded }: { expanded: boolean }) {
  const notice = useCurrentAskNotice();
  const { notify } = useInteraction('ask-errors');
  useEffect(() => {
    if (!expanded || !notice) return;
    const handle = notify({
      key: 'ask-error', message: notice.message, variant: notice.severity, durationMs: 4500,
    });
    return handle.dismiss;
  }, [expanded, notice, notify]);
  return null;
}

export function AskNotice({ message, severity, operation }: {
  message?: unknown; severity: Severity; operation?: string;
}) {
  const report = useAskNotice();
  useEffect(() => {
    if (message) report(message, severity, operation);
  }, [message, severity, operation, report]);
  return null;
}
