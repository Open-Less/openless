import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckIcon, PencilLine } from 'lucide-react';
import { ToolWindowHeader } from '../components/ui/ToolWindowHeader';
import {
  cancelSelectionPolishPreview,
  confirmSelectionPolishPreview,
  getSelectionPolishPreview,
  isTauri,
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
    if (!isTauri)
      return () => {
        cancelled = true;
      };
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
    <main className="ol-tool-window">
      <ToolWindowHeader
        icon={<PencilLine />}
        title={t('selectionPolishPreview.title')}
        description={t('selectionPolishPreview.subtitle')}
        onClose={() => void cancel()}
        closeLabel={t('selectionPolishPreview.cancel')}
        closeDisabled={busy}
      />
      <section className="ol-tool-content">
        <textarea
          className="ol-tool-editor"
          aria-label={t('selectionPolishPreview.resultLabel')}
          autoFocus
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
        {sourceText && (
          <div className="ol-tool-source">
            {t('selectionPolishPreview.sourcePrefix')}
            {sourceText}
          </div>
        )}
        {error && (
          <div className="ol-tool-error" role="alert">
            {t('selectionPolishPreview.applyError')}
            {error}
          </div>
        )}
      </section>
      <footer className="ol-tool-footer">
        <button className="ol-tool-button" onClick={() => void cancel()} disabled={busy}>
          {t('selectionPolishPreview.cancel')}
        </button>
        <button
          className="ol-tool-button is-primary"
          onClick={() => void confirm()}
          disabled={busy || !text.trim()}
        >
          <CheckIcon size={16} />
          {t('selectionPolishPreview.confirmReplace')}
        </button>
      </footer>
    </main>
  );
}
