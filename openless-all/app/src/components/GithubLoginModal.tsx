// GitHub login modal — the style marketplace and extension marketplace share one
// login UI. GitHub OAuth Device Flow: start on open → show the user code and wait
// for browser authorization → poll until authorized. All phases share one minHeight
// container so the window size stays constant — no more small-then-larger jump.

import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CircleCheckIcon, XIcon } from 'lucide-react';
import {
  githubDeviceFlowCancel,
  githubDeviceFlowPoll,
  githubDeviceFlowStart,
  githubFlowExpiresAt,
  githubPollIntervalMs,
  githubSlowDownIntervalMs,
  openExternal,
} from '../lib/ipc';
import { Btn } from '../pages/_atoms';
import { Modal } from './ui/Modal';

type Phase =
  | { kind: 'starting' }
  | {
      kind: 'pending';
      userCode: string;
      verificationUri: string;
      flowId: string;
      intervalMs: number;
      expiresAt: number;
    }
  | { kind: 'success'; login: string }
  | { kind: 'error'; message: string };

interface GithubLoginModalProps {
  onClose: () => void;
  /** Called on successful authorization (receives the GitHub login). */
  onSuccess: (login: string) => void;
  /** Exit animation driven by the caller's useExitMount. */
  closing?: boolean;
  overlayClassName?: string;
}

export function GithubLoginModal({
  onClose,
  onSuccess,
  closing = false,
  overlayClassName,
}: GithubLoginModalProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const [phase, setPhase] = useState<Phase>({ kind: 'starting' });
  const [copied, setCopied] = useState(false);
  const cancelledRef = useRef(false);
  const beginGenerationRef = useRef(0);
  const activeFlowIdRef = useRef<string | undefined>(undefined);
  // Hold callbacks in refs so the poll effect depends only on phase and doesn't
  // restart on parent re-renders.
  const onSuccessRef = useRef(onSuccess);
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onSuccessRef.current = onSuccess;
    onCloseRef.current = onClose;
  });

  const cancelActiveFlow = useCallback(async () => {
    const flowId = activeFlowIdRef.current;
    activeFlowIdRef.current = undefined;
    try {
      await githubDeviceFlowCancel(flowId);
    } catch {
      /* best effort */
    }
  }, []);

  const begin = useCallback(async () => {
    const generation = ++beginGenerationRef.current;
    cancelledRef.current = false;
    setPhase({ kind: 'starting' });
    try {
      await cancelActiveFlow();
      if (cancelledRef.current || generation !== beginGenerationRef.current) return;
      const start = await githubDeviceFlowStart();
      if (cancelledRef.current || generation !== beginGenerationRef.current) {
        await githubDeviceFlowCancel(start.flowId).catch(() => undefined);
        return;
      }
      activeFlowIdRef.current = start.flowId;
      setPhase({
        kind: 'pending',
        userCode: start.userCode,
        verificationUri: start.verificationUri,
        flowId: start.flowId,
        intervalMs: githubPollIntervalMs(start.interval),
        expiresAt: githubFlowExpiresAt(Date.now(), start.expiresIn),
      });
      // Open the browser automatically; failure is non-fatal, the user can copy manually.
      try {
        await openExternal(start.verificationUri);
      } catch {
        /* manual fallback */
      }
    } catch (err) {
      if (cancelledRef.current) return;
      setPhase({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [cancelActiveFlow]);

  // Start the login immediately on open.
  useEffect(() => {
    void begin();
    return () => {
      cancelledRef.current = true;
      beginGenerationRef.current += 1;
      void cancelActiveFlow();
    };
  }, [begin, cancelActiveFlow]);

  // Poll the backend during the pending phase.
  useEffect(() => {
    if (phase.kind !== 'pending') return;
    let cancelled = false;
    let timer: number | null = null;
    let interval = phase.intervalMs;
    const { flowId, expiresAt } = phase;
    const tick = async () => {
      if (cancelled) return;
      if (Date.now() >= expiresAt) {
        await githubDeviceFlowCancel(flowId).catch(() => undefined);
        activeFlowIdRef.current = undefined;
        if (!cancelled) {
          setPhase({
            kind: 'error',
            message: t('marketplace.oauth.expiredError', {
              defaultValue: 'GitHub authorization expired. Start again.',
            }),
          });
        }
        return;
      }
      try {
        const res = await githubDeviceFlowPoll(flowId);
        if (cancelled) return;
        if (res.kind === 'authorized') {
          activeFlowIdRef.current = undefined;
          setPhase({ kind: 'success', login: res.login });
          onSuccessRef.current(res.login);
          window.setTimeout(() => {
            if (!cancelled) onCloseRef.current();
          }, 1200);
        } else if (res.kind === 'slowDown') {
          interval = githubSlowDownIntervalMs(interval);
          timer = window.setTimeout(tick, Math.min(interval, Math.max(0, expiresAt - Date.now())));
        } else if (res.kind === 'pending') {
          timer = window.setTimeout(tick, Math.min(interval, Math.max(0, expiresAt - Date.now())));
        } else {
          activeFlowIdRef.current = undefined;
          setPhase({ kind: 'error', message: res.message });
        }
      } catch (err) {
        if (cancelled) return;
        setPhase({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      }
    };
    timer = window.setTimeout(tick, interval);
    return () => {
      cancelled = true;
      if (timer != null) window.clearTimeout(timer);
    };
  }, [phase, t]);

  const close = () => {
    cancelledRef.current = true;
    beginGenerationRef.current += 1;
    void cancelActiveFlow().finally(onClose);
  };

  const copyCode = async () => {
    if (phase.kind !== 'pending') return;
    try {
      await navigator.clipboard.writeText(phase.userCode);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  };

  return (
    <Modal
      onClose={close}
      zIndex={60}
      width="min(440px, 100%)"
      closing={closing}
      overlayClassName={overlayClassName}
      labelledBy={titleId}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 14,
          gap: 12,
        }}
      >
        <h2 id={titleId} style={{ margin: 0, fontSize: 16, fontWeight: 650 }}>
          {t('marketplace.oauth.title')}
        </h2>
        <button
          type="button"
          aria-label={t('common.close')}
          title={t('common.close')}
          onClick={close}
          style={{
            width: 28,
            height: 28,
            borderRadius: 999,
            display: 'inline-grid',
            placeItems: 'center',
            border: '0.5px solid var(--ol-line-strong)',
            background: 'var(--ol-surface)',
            color: 'var(--ol-ink-2)',
            cursor: 'pointer',
          }}
        >
          <XIcon size={15} strokeWidth={2} aria-hidden />
        </button>
      </div>

      {/* Fixed min height — shared by all phases, keeping the window size constant. */}
      <div style={{ minHeight: 220, display: 'flex', flexDirection: 'column' }}>
        {phase.kind === 'starting' && (
          <div
            style={{
              flex: 1,
              display: 'grid',
              placeItems: 'center',
              color: 'var(--ol-ink-3)',
              fontSize: 13,
            }}
          >
            {t('marketplace.oauth.generating')}
          </div>
        )}

        {phase.kind === 'pending' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
            <div style={{ fontSize: 13, color: 'var(--ol-ink-2)', lineHeight: 1.6 }}>
              {t('marketplace.oauth.browserHint', { uri: phase.verificationUri })}
            </div>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: 12,
                padding: 18,
                borderRadius: 12,
                border: '0.5px solid var(--ol-line-strong)',
                background: 'var(--ol-surface-2)',
              }}
            >
              <span
                style={{
                  fontFamily: 'var(--ol-font-mono)',
                  fontSize: 22,
                  fontWeight: 700,
                  letterSpacing: 2,
                  color: 'var(--ol-blue)',
                }}
              >
                {phase.userCode}
              </span>
              <Btn variant="ghost" size="sm" onClick={() => void copyCode()}>
                {copied ? t('marketplace.oauth.copied') : t('marketplace.oauth.copyBtn')}
              </Btn>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
              <Btn
                variant="ghost"
                size="sm"
                onClick={() => void openExternal(phase.verificationUri)}
              >
                {t('marketplace.oauth.openBrowserBtn')}
              </Btn>
              <Btn variant="ghost" size="sm" onClick={close}>
                {t('marketplace.oauth.cancelBtn')}
              </Btn>
            </div>
            <div
              style={{
                fontSize: 11.5,
                color: 'var(--ol-ink-4)',
                textAlign: 'center',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: 6,
              }}
            >
              <span
                style={{
                  display: 'inline-block',
                  width: 8,
                  height: 8,
                  borderRadius: 999,
                  background: 'var(--ol-blue)',
                  animation: 'ol-pulse 1.4s ease-in-out infinite',
                }}
              />
              {t('marketplace.oauth.waiting')}
            </div>
            <style>{`@keyframes ol-pulse { 0%, 100% { opacity: 0.3; } 50% { opacity: 1; } }`}</style>
          </div>
        )}

        {phase.kind === 'success' && (
          <div style={{ flex: 1, display: 'grid', placeItems: 'center', textAlign: 'center' }}>
            <div style={{ display: 'grid', justifyItems: 'center' }}>
              <CircleCheckIcon
                size={30}
                strokeWidth={1.8}
                aria-hidden
                style={{ color: 'var(--ol-ok, var(--ol-blue))', marginBottom: 10 }}
              />
              <div style={{ fontSize: 14, fontWeight: 650, color: 'var(--ol-ink)' }}>
                {t('marketplace.oauth.successAs', { login: phase.login })}
              </div>
            </div>
          </div>
        )}

        {phase.kind === 'error' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div
              style={{
                padding: 12,
                borderRadius: 10,
                border: '0.5px solid color-mix(in srgb, var(--ol-err) 32%, transparent)',
                background: 'color-mix(in srgb, var(--ol-err) 8%, transparent)',
                color: 'var(--ol-err)',
                fontSize: 12,
                lineHeight: 1.6,
                whiteSpace: 'pre-wrap',
              }}
            >
              {phase.message}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
              <Btn variant="ghost" size="sm" onClick={close}>
                {t('marketplace.oauth.closeBtn')}
              </Btn>
              <Btn variant="blue" size="sm" onClick={() => void begin()}>
                {t('marketplace.oauth.retryBtn')}
              </Btn>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
