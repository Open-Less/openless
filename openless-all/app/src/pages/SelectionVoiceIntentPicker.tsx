import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { MessageCircleQuestion, PencilLine, Sparkles } from 'lucide-react';
import { ToolWindowHeader } from '../components/ui/ToolWindowHeader';
import {
  cancelSelectionVoiceIntentPrompt,
  confirmSelectionVoiceIntentPrompt,
  getSelectionVoiceIntentPrompt,
  isTauri,
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
    if (!isTauri)
      return () => {
        cancelled = true;
      };
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen('selection-voice-intent:shown', () => {
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
    <main className="ol-tool-window ol-intent-window">
      <ToolWindowHeader
        icon={<Sparkles />}
        title={t('selectionVoiceIntent.title')}
        description={t('selectionVoiceIntent.subtitle')}
        onClose={() => void cancel()}
        closeLabel={t('selectionVoiceIntent.cancel')}
        closeDisabled={busy}
      />
      <section className="ol-tool-content">
        <div className="ol-intent-instruction">
          {instruction || t('selectionVoiceIntent.loading')}
        </div>
        {sourceText && (
          <div className="ol-tool-source">
            {t('selectionVoiceIntent.sourcePrefix')}
            {sourceText}
          </div>
        )}
        {error && (
          <div className="ol-tool-error" role="alert">
            {t('selectionVoiceIntent.errorPrefix')}
            {error}
          </div>
        )}
      </section>
      <footer className="ol-intent-footer">
        <div className="ol-intent-options">
          <button
            className="ol-tool-button"
            disabled={busy}
            onClick={() => void choose('question')}
          >
            <MessageCircleQuestion size={20} />
            {t('selectionVoiceIntent.question')}
          </button>
          <button
            className="ol-tool-button is-primary"
            disabled={busy}
            onClick={() => void choose('edit')}
          >
            <PencilLine size={20} />
            {t('selectionVoiceIntent.edit')}
          </button>
        </div>
        <button
          className="ol-tool-button ol-intent-cancel"
          disabled={busy}
          onClick={() => void cancel()}
        >
          {t('selectionVoiceIntent.cancel')}
        </button>
      </footer>
    </main>
  );
}
