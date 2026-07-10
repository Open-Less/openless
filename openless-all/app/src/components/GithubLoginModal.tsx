// GitHub 登录弹窗 —— 风格市场与扩展市场共用同一套登录界面。
// GitHub OAuth Device Flow：打开即 start → 展示 user code 等浏览器授权 →
// 轮询直到 authorized。各阶段内容套同一 minHeight 容器，窗口尺寸恒定，
// 不再出现「先弹小窗、过会儿变大窗」的跳动。

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  githubDeviceFlowPoll,
  githubDeviceFlowStart,
  openExternal,
} from '../lib/ipc';
import { Btn } from '../pages/_atoms';
import { Modal } from './ui/Modal';

type Phase =
  | { kind: 'starting' }
  | { kind: 'pending'; userCode: string; verificationUri: string; deviceCode: string }
  | { kind: 'success'; login: string }
  | { kind: 'error'; message: string };

interface GithubLoginModalProps {
  onClose: () => void;
  /** 授权成功回调（拿到 GitHub login）。 */
  onSuccess: (login: string) => void;
}

export function GithubLoginModal({ onClose, onSuccess }: GithubLoginModalProps) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>({ kind: 'starting' });
  const [copied, setCopied] = useState(false);
  const cancelledRef = useRef(false);
  // 用 ref 持有回调，poll 副作用只依赖 phase，不因父组件重渲染而重启。
  const onSuccessRef = useRef(onSuccess);
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onSuccessRef.current = onSuccess;
    onCloseRef.current = onClose;
  });

  const begin = useCallback(async () => {
    cancelledRef.current = false;
    setPhase({ kind: 'starting' });
    try {
      const start = await githubDeviceFlowStart();
      if (cancelledRef.current) return;
      setPhase({
        kind: 'pending',
        userCode: start.userCode,
        verificationUri: start.verificationUri,
        deviceCode: start.deviceCode,
      });
      // 自动拉起浏览器；失败不致命，用户可手动复制。
      try { await openExternal(start.verificationUri); } catch { /* manual fallback */ }
    } catch (err) {
      if (cancelledRef.current) return;
      setPhase({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, []);

  // 打开即发起登录。
  useEffect(() => {
    void begin();
    return () => { cancelledRef.current = true; };
  }, [begin]);

  // pending 阶段轮询 backend。
  useEffect(() => {
    if (phase.kind !== 'pending') return;
    let cancelled = false;
    let timer: number | null = null;
    let interval = 5_000;
    const deviceCode = phase.deviceCode;
    const tick = async () => {
      if (cancelled) return;
      try {
        const res = await githubDeviceFlowPoll(deviceCode);
        if (cancelled) return;
        if (res.kind === 'authorized') {
          setPhase({ kind: 'success', login: res.login });
          onSuccessRef.current(res.login);
          window.setTimeout(() => { if (!cancelled) onCloseRef.current(); }, 1200);
        } else if (res.kind === 'slowDown') {
          interval = Math.min(interval + 5_000, 30_000);
          timer = window.setTimeout(tick, interval);
        } else if (res.kind === 'pending') {
          timer = window.setTimeout(tick, interval);
        } else {
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
  }, [phase]);

  const close = () => {
    cancelledRef.current = true;
    onClose();
  };

  const copyCode = async () => {
    if (phase.kind !== 'pending') return;
    try {
      await navigator.clipboard.writeText(phase.userCode);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch { /* clipboard unavailable */ }
  };

  return (
    <Modal onClose={close} zIndex={60} width="min(440px, 100%)">
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14, gap: 12 }}>
        <h2 style={{ margin: 0, fontSize: 16, fontWeight: 650 }}>{t('marketplace.oauth.title')}</h2>
        <button
          type="button"
          aria-label={t('common.close')}
          title={t('common.close')}
          onClick={close}
          style={{
            width: 28, height: 28, borderRadius: 8,
            display: 'inline-grid', placeItems: 'center',
            border: '0.5px solid var(--ol-line-strong)',
            background: 'var(--ol-surface)',
            color: 'var(--ol-ink-2)',
            cursor: 'pointer',
            fontSize: 16, lineHeight: 1,
          }}
        >×</button>
      </div>

      {/* 固定最小高度 —— 各阶段共用，窗口尺寸恒定。 */}
      <div style={{ minHeight: 220, display: 'flex', flexDirection: 'column' }}>
        {phase.kind === 'starting' && (
          <div style={{ flex: 1, display: 'grid', placeItems: 'center', color: 'var(--ol-ink-3)', fontSize: 13 }}>
            {t('marketplace.oauth.generating')}
          </div>
        )}

        {phase.kind === 'pending' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
            <div style={{ fontSize: 13, color: 'var(--ol-ink-2)', lineHeight: 1.6 }}>
              {t('marketplace.oauth.browserHint', { uri: phase.verificationUri })}
            </div>
            <div style={{
              display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 12,
              padding: 18, borderRadius: 12,
              border: '0.5px solid var(--ol-line-strong)',
              background: 'var(--ol-surface-2)',
            }}>
              <span style={{
                fontFamily: 'var(--ol-font-mono)',
                fontSize: 22, fontWeight: 700,
                letterSpacing: 2,
                color: 'var(--ol-blue)',
              }}>{phase.userCode}</span>
              <Btn variant="ghost" size="sm" onClick={() => void copyCode()}>
                {copied ? t('marketplace.oauth.copied') : t('marketplace.oauth.copyBtn')}
              </Btn>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
              <Btn variant="ghost" size="sm" onClick={() => void openExternal(phase.verificationUri)}>
                {t('marketplace.oauth.openBrowserBtn')}
              </Btn>
              <Btn variant="ghost" size="sm" onClick={close}>
                {t('marketplace.oauth.cancelBtn')}
              </Btn>
            </div>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', textAlign: 'center', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6 }}>
              <span style={{
                display: 'inline-block', width: 8, height: 8, borderRadius: 999,
                background: 'var(--ol-blue)', animation: 'ol-pulse 1.4s ease-in-out infinite',
              }} />
              {t('marketplace.oauth.waiting')}
            </div>
            <style>{`@keyframes ol-pulse { 0%, 100% { opacity: 0.3; } 50% { opacity: 1; } }`}</style>
          </div>
        )}

        {phase.kind === 'success' && (
          <div style={{ flex: 1, display: 'grid', placeItems: 'center', textAlign: 'center' }}>
            <div>
              <div style={{ fontSize: 24, color: 'var(--ol-blue)', marginBottom: 8 }}>✓</div>
              <div style={{ fontSize: 14, fontWeight: 650, color: 'var(--ol-ink)' }}>
                {t('marketplace.oauth.successAs', { login: phase.login })}
              </div>
            </div>
          </div>
        )}

        {phase.kind === 'error' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{
              padding: 12, borderRadius: 10,
              border: '0.5px solid rgba(239,68,68,0.3)',
              background: 'rgba(239,68,68,0.06)',
              color: '#b91c1c',
              fontSize: 12, lineHeight: 1.6,
              whiteSpace: 'pre-wrap',
            }}>
              {phase.message}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
              <Btn variant="ghost" size="sm" onClick={close}>{t('marketplace.oauth.closeBtn')}</Btn>
              <Btn variant="blue" size="sm" onClick={() => void begin()}>{t('marketplace.oauth.retryBtn')}</Btn>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
