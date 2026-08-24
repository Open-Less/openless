import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LoaderCircle, MessageSquareText, Replace, Square, X } from 'lucide-react';
import {
  cancelSelectionCorrection,
  dismissSelectionCorrection,
  getSelectionCorrection,
  startSelectionCorrection,
  stopSelectionCorrection,
  type SelectionCorrectionAction,
  type SelectionCorrectionBubblePayload,
} from '../lib/ipc/selection-correction';

export function SelectionCorrectionBubble() {
  const { t } = useTranslation();
  const [payload, setPayload] = useState<SelectionCorrectionBubblePayload | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getSelectionCorrection().then(value => {
      if (!cancelled) setPayload(value);
    });
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen<SelectionCorrectionBubblePayload>('selection-correction:state', event => {
        if (!cancelled) {
          setPayload(event.payload);
          setBusy(false);
        }
      }).then(handle => {
        if (cancelled) handle(); else unlisten = handle;
      }),
    );
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      if (payload?.state === 'recording' || payload?.state === 'processing') {
        void cancelSelectionCorrection();
      } else {
        void dismissSelectionCorrection();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      cancelled = true;
      unlisten?.();
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [payload?.state]);

  const start = async (action: SelectionCorrectionAction) => {
    if (busy) return;
    setBusy(true);
    try {
      await startSelectionCorrection(action);
    } catch (error) {
      setPayload(current => current ? {
        ...current,
        state: 'error',
        action,
        message: String(error),
      } : current);
      setBusy(false);
    }
  };

  const stop = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await stopSelectionCorrection();
    } catch (error) {
      setPayload(current => current ? { ...current, state: 'error', message: String(error) } : current);
      setBusy(false);
    }
  };

  if (!payload) return null;
  const selected = payload.selectedText.length > 30
    ? `${payload.selectedText.slice(0, 29)}…`
    : payload.selectedText;

  return (
    <main style={{ height: '100vh', padding: 7, boxSizing: 'border-box', background: 'transparent', color: 'var(--ol-ink)' }}>
      <section style={{ height: '100%', boxSizing: 'border-box', borderRadius: 12, border: '0.5px solid var(--ol-line-strong)', background: 'color-mix(in srgb, var(--ol-surface) 94%, transparent)', boxShadow: '0 12px 34px rgba(0,0,0,.18)', backdropFilter: 'blur(22px)', padding: '10px 11px' }}>
        <header style={{ display: 'flex', alignItems: 'center', minWidth: 0, gap: 7, height: 22 }}>
          <span style={{ minWidth: 0, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: 12, color: 'var(--ol-ink-3)' }} title={payload.selectedText}>
            “{selected}”
          </span>
          <button
            className="ol-focus-ring"
            aria-label={t('selectionCorrection.close', '关闭')}
            onClick={() => void (payload.state === 'recording' || payload.state === 'processing' ? cancelSelectionCorrection() : dismissSelectionCorrection())}
            style={{ width: 22, height: 22, display: 'grid', placeItems: 'center', borderRadius: 6, color: 'var(--ol-ink-4)' }}
          >
            <X size={14} />
          </button>
        </header>

        {payload.state === 'actions' && (
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 7, marginTop: 7 }}>
            <ActionButton icon={<Replace size={15} />} label={t('selectionCorrection.replace', '直接替换')} disabled={busy} onClick={() => void start('literalReplace')} />
            <ActionButton icon={<MessageSquareText size={15} />} label={t('selectionCorrection.review', '批注修改')} disabled={busy} onClick={() => void start('review')} />
          </div>
        )}

        {payload.state === 'recording' && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 7 }}>
            <div style={{ flex: 1, display: 'flex', alignItems: 'center', gap: 8, height: 34, padding: '0 10px', borderRadius: 8, background: 'var(--ol-control-solid)', fontSize: 12, color: 'var(--ol-ink-2)' }}>
              <span style={{ width: 7, height: 7, borderRadius: '50%', background: 'var(--ol-red, #ef4444)', boxShadow: '0 0 0 4px color-mix(in srgb, var(--ol-red, #ef4444) 15%, transparent)' }} />
              {payload.action === 'review'
                ? t('selectionCorrection.recordReview', '请说出批注意见')
                : t('selectionCorrection.recordReplacement', '请说出替换内容')}
            </div>
            <button className="ol-focus-ring" disabled={busy} onClick={() => void stop()} style={{ width: 38, height: 34, display: 'grid', placeItems: 'center', borderRadius: 8, background: 'var(--ol-ink)', color: 'var(--ol-surface)' }} title={t('selectionCorrection.finish', '完成')}>
              <Square size={13} fill="currentColor" />
            </button>
          </div>
        )}

        {payload.state === 'processing' && (
          <div style={{ height: 41, display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, color: 'var(--ol-ink-3)' }}>
            <LoaderCircle size={15} style={{ animation: 'spin 1s linear infinite' }} />
            {payload.action === 'review'
              ? t('selectionCorrection.generating', '正在生成修改建议…')
              : t('selectionCorrection.transcribing', '正在识别替换内容…')}
          </div>
        )}

        {payload.state === 'error' && (
          <div style={{ height: 41, display: 'flex', alignItems: 'center', fontSize: 11, color: 'var(--ol-red, #dc2626)', overflow: 'hidden' }} title={payload.message ?? undefined}>
            {t('selectionCorrection.failed', '操作失败，请重新选择后再试')}
          </div>
        )}
      </section>
    </main>
  );
}

function ActionButton({ icon, label, disabled, onClick }: { icon: React.ReactNode; label: string; disabled: boolean; onClick: () => void }) {
  return (
    <button
      className="ol-focus-ring"
      disabled={disabled}
      onClick={onClick}
      style={{ height: 35, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 7, borderRadius: 8, border: '0.5px solid var(--ol-line-strong)', background: 'var(--ol-control-solid)', color: 'var(--ol-ink-2)', fontSize: 12, fontWeight: 600 }}
    >
      {icon}{label}
    </button>
  );
}
