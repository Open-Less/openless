import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState, type ReactNode } from 'react';

type Severity = 'warning' | 'error';
type Notice = { id: number; message: string; severity: Severity };
type Report = (message: string, severity: Severity, key?: string) => { dismiss(): void };
const AskNoticeContext = createContext<{
  notice?: Notice;
  report: Report;
  clear(): void;
  scope(): number;
}>({ report: () => ({ dismiss() {} }), clear() {}, scope: () => 0 });

export function AskNoticeProvider({ children }: { children: ReactNode }) {
  const [notice, setNotice] = useState<Notice>();
  const next = useRef(0);
  const session = useRef(0);
  const scope = useCallback(() => session.current, []);
  const clear = useCallback(() => {
    session.current++;
    setNotice(undefined);
  }, []);
  const report = useCallback<Report>((message, severity) => {
    const id = ++next.current;
    setNotice({ id, message, severity });
    return { dismiss: () => setNotice((current) => (current?.id === id ? undefined : current)) };
  }, []);
  return <AskNoticeContext.Provider value={{ notice, report, clear, scope }}>{children}</AskNoticeContext.Provider>;
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

export function useScopedAskNotice() {
  const report = useAskNotice();
  const { scope } = useContext(AskNoticeContext);
  const generation = useRef(scope());
  return useCallback<Report>((...args) =>
    scope() === generation.current ? report(...args) : { dismiss() {} }, [report, scope]);
}

export function AskCurrentNotice() {
  const notice = useContext(AskNoticeContext).notice;
  return <AskNotice message={notice?.message} severity={notice?.severity ?? 'warning'} />;
}

export function AskNotice({ message, severity }: { message?: string; severity: Severity }) {
  return message ? (
    <p className="ask-inline-notice" data-severity={severity} role="alert">{message}</p>
  ) : null;
}
