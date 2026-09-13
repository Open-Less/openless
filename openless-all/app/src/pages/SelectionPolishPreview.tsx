import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckIcon, XIcon } from 'lucide-react';
import {
  cancelSelectionPolishPreview,
  confirmSelectionPolishPreview,
  getSelectionPolishPreview,
} from '../lib/ipc';

export function SelectionPolishPreview() {
  const { t } = useTranslation();
  const [text, setText] = useState('');
  const [sourceText, setSourceText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const load = async () => {
      const preview = await getSelectionPolishPreview();
      if (!cancelled && preview) {
        setText(preview.text);
        setSourceText(preview.sourceText);
        setError(null);
      }
    };
    void load();
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen('selection-polish-preview:shown', () => {
        // 预览窗是复用的：上一轮 confirm/cancel 成功后窗口 hide，但组件不卸载，
        // busy 会停留在 true → 下一轮两个按钮全 disabled（表现为「点确认没反应」）。
        // 每次重新 show 必须复位交互状态。
        setBusy(false);
        setError(null);
        void load();
      }).then((handle) => {
        if (cancelled) handle();
        else unlisten = handle;
      }),
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const cancel = async () => {
    setBusy(true);
    await cancelSelectionPolishPreview();
  };
  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await confirmSelectionPolishPreview(text);
    } catch (reason) {
      setError(String(reason));
      setBusy(false);
    }
  };

  return (
    <main
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        boxSizing: 'border-box',
        padding: 18,
        background: 'var(--ol-surface)',
        color: 'var(--ol-ink)',
      }}
    >
      <header
        style={{
          display: 'flex',
          alignItems: 'flex-start',
          justifyContent: 'space-between',
          gap: 12,
          marginBottom: 12,
        }}
      >
        <div>
          <div style={{ fontSize: 16, fontWeight: 700 }}>{t('selectionPolishPreview.title')}</div>
          <div style={{ marginTop: 4, fontSize: 12, color: 'var(--ol-ink-4)' }}>
            {t('selectionPolishPreview.subtitle')}
          </div>
        </div>
        <button
          className="ol-focus-ring"
          onClick={() => void cancel()}
          disabled={busy}
          title={t('selectionPolishPreview.cancel')}
          style={{ width: 30, height: 30, borderRadius: 7, color: 'var(--ol-ink-3)' }}
        >
          <XIcon size={17} />
        </button>
      </header>
      <textarea
        aria-label={t('selectionPolishPreview.resultLabel')}
        autoFocus
        value={text}
        onChange={(event) => setText(event.target.value)}
        style={{
          flex: 1,
          minHeight: 150,
          width: '100%',
          resize: 'none',
          boxSizing: 'border-box',
          border: '0.5px solid var(--ol-line-strong)',
          borderRadius: 9,
          background: 'var(--ol-control-solid)',
          color: 'var(--ol-ink)',
          padding: 12,
          fontSize: 14,
          lineHeight: 1.65,
          outline: 'none',
        }}
      />
      {sourceText && (
        <div
          style={{
            marginTop: 8,
            maxHeight: 42,
            overflow: 'hidden',
            fontSize: 11,
            lineHeight: 1.5,
            color: 'var(--ol-ink-4)',
          }}
        >
          {t('selectionPolishPreview.sourcePrefix')}
          {sourceText}
        </div>
      )}
      {error && (
        <div style={{ marginTop: 8, fontSize: 12, color: 'var(--ol-red, #dc2626)' }}>
          {t('selectionPolishPreview.applyError')}
          {error}
        </div>
      )}
      <footer style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 14 }}>
        <button
          className="ol-focus-ring"
          onClick={() => void cancel()}
          disabled={busy}
          style={{
            padding: '8px 14px',
            borderRadius: 7,
            border: '0.5px solid var(--ol-line-strong)',
            color: 'var(--ol-ink-2)',
          }}
        >
          {t('selectionPolishPreview.cancel')}
        </button>
        <button
          className="ol-focus-ring"
          onClick={() => void confirm()}
          disabled={busy || !text.trim()}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            padding: '8px 14px',
            borderRadius: 7,
            background: 'var(--ol-blue)',
            color: '#fff',
            fontWeight: 600,
          }}
        >
          <CheckIcon size={16} />
          {t('selectionPolishPreview.confirmReplace')}
        </button>
      </footer>
    </main>
  );
}
