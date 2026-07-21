import { useEffect, useState } from 'react';
import { CheckIcon, XIcon } from 'lucide-react';
import {
  cancelSelectionPolishPreview,
  confirmSelectionPolishPreview,
  getSelectionPolishPreview,
} from '../lib/ipc';

export function SelectionPolishPreview() {
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
      listen('selection-polish-preview:shown', () => { void load(); }).then(handle => {
        if (cancelled) handle(); else unlisten = handle;
      }),
    );
    return () => { cancelled = true; unlisten?.(); };
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
    <main style={{ display: 'flex', flexDirection: 'column', height: '100%', boxSizing: 'border-box', padding: 18, background: 'var(--ol-surface)', color: 'var(--ol-ink)' }}>
      <header style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, marginBottom: 12 }}>
        <div>
          <div style={{ fontSize: 16, fontWeight: 700 }}>选区润色预览</div>
          <div style={{ marginTop: 4, fontSize: 12, color: 'var(--ol-ink-4)' }}>可直接编辑；点击确认后才会替换原选区。</div>
        </div>
        <button className="ol-focus-ring" onClick={() => void cancel()} disabled={busy} title="取消" style={{ width: 30, height: 30, borderRadius: 7, color: 'var(--ol-ink-3)' }}><XIcon size={17} /></button>
      </header>
      <textarea
        aria-label="润色结果"
        autoFocus
        value={text}
        onChange={event => setText(event.target.value)}
        style={{ flex: 1, minHeight: 150, width: '100%', resize: 'none', boxSizing: 'border-box', border: '0.5px solid var(--ol-line-strong)', borderRadius: 9, background: 'var(--ol-control-solid)', color: 'var(--ol-ink)', padding: 12, fontSize: 14, lineHeight: 1.65, outline: 'none' }}
      />
      {sourceText && <div style={{ marginTop: 8, maxHeight: 42, overflow: 'hidden', fontSize: 11, lineHeight: 1.5, color: 'var(--ol-ink-4)' }}>原文：{sourceText}</div>}
      {error && <div style={{ marginTop: 8, fontSize: 12, color: 'var(--ol-red, #dc2626)' }}>未能应用：{error}</div>}
      <footer style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 14 }}>
        <button className="ol-focus-ring" onClick={() => void cancel()} disabled={busy} style={{ padding: '8px 14px', borderRadius: 7, border: '0.5px solid var(--ol-line-strong)', color: 'var(--ol-ink-2)' }}>取消</button>
        <button className="ol-focus-ring" onClick={() => void confirm()} disabled={busy || !text.trim()} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, padding: '8px 14px', borderRadius: 7, background: 'var(--ol-blue)', color: '#fff', fontWeight: 600 }}><CheckIcon size={16} />确认并替换</button>
      </footer>
    </main>
  );
}
