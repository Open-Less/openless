import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { MessageCircleQuestion, PencilLine } from 'lucide-react';
import {
  cancelSelectionVoiceIntentPrompt,
  confirmSelectionVoiceIntentPrompt,
  getSelectionVoiceIntentPrompt,
} from '../lib/ipc';

export function SelectionVoiceIntentPicker() {
  const { t } = useTranslation();
  const [instruction, setInstruction] = useState('');
  const [sourceText, setSourceText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const load = async () => {
      const prompt = await getSelectionVoiceIntentPrompt();
      if (!cancelled && prompt) {
        setInstruction(prompt.instruction);
        setSourceText(prompt.sourceText);
        setError(null);
      }
    };
    void load();
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen('selection-voice-intent:shown', () => { void load(); }).then(handle => {
        if (cancelled) handle(); else unlisten = handle;
      }),
    );
    return () => { cancelled = true; unlisten?.(); };
  }, []);

  const choose = async (intent: 'question' | 'edit') => {
    setBusy(true);
    setError(null);
    try {
      await confirmSelectionVoiceIntentPrompt(intent);
    } catch (reason) {
      setError(String(reason));
      setBusy(false);
    }
  };

  const cancel = async () => {
    setBusy(true);
    await cancelSelectionVoiceIntentPrompt();
  };

  return (
    <main style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      boxSizing: 'border-box',
      padding: 18,
      background: 'var(--ol-surface)',
      color: 'var(--ol-ink)',
    }}
    >
      <header style={{ marginBottom: 12 }}>
        <div style={{ fontSize: 16, fontWeight: 700 }}>{t('selectionVoiceIntent.title')}</div>
        <div style={{ marginTop: 4, fontSize: 12, color: 'var(--ol-ink-4)' }}>
          {t('selectionVoiceIntent.subtitle')}
        </div>
      </header>
      <div style={{
        padding: '10px 12px',
        borderRadius: 9,
        border: '0.5px solid var(--ol-line-strong)',
        background: 'var(--ol-control-solid)',
        fontSize: 14,
        lineHeight: 1.6,
        minHeight: 48,
      }}
      >
        {instruction || t('selectionVoiceIntent.loading')}
      </div>
      {sourceText && (
        <div style={{
          marginTop: 8,
          maxHeight: 40,
          overflow: 'hidden',
          fontSize: 11,
          lineHeight: 1.5,
          color: 'var(--ol-ink-4)',
        }}
        >
          {t('selectionVoiceIntent.sourcePrefix')}{sourceText}
        </div>
      )}
      {error && (
        <div style={{ marginTop: 8, fontSize: 12, color: 'var(--ol-red, #dc2626)' }}>
          {t('selectionVoiceIntent.errorPrefix')}{error}
        </div>
      )}
      <footer style={{ display: 'flex', gap: 8, marginTop: 16 }}>
        <button
          className="ol-focus-ring"
          disabled={busy}
          onClick={() => void choose('question')}
          style={{
            flex: 1,
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 8,
            padding: '12px 10px',
            borderRadius: 9,
            border: '0.5px solid var(--ol-line-strong)',
            background: 'var(--ol-control-solid)',
            fontWeight: 600,
          }}
        >
          <MessageCircleQuestion size={18} />
          {t('selectionVoiceIntent.question')}
        </button>
        <button
          className="ol-focus-ring"
          disabled={busy}
          onClick={() => void choose('edit')}
          style={{
            flex: 1,
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 8,
            padding: '12px 10px',
            borderRadius: 9,
            background: 'var(--ol-blue)',
            color: '#fff',
            fontWeight: 600,
          }}
        >
          <PencilLine size={18} />
          {t('selectionVoiceIntent.edit')}
        </button>
      </footer>
      <button
        className="ol-focus-ring"
        disabled={busy}
        onClick={() => void cancel()}
        style={{
          marginTop: 10,
          alignSelf: 'center',
          fontSize: 12,
          color: 'var(--ol-ink-4)',
          padding: '4px 8px',
        }}
      >
        {t('selectionVoiceIntent.cancel')}
      </button>
    </main>
  );
}
