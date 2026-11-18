import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { MessageCircleQuestion, PencilLine, PenLine, Sparkles } from 'lucide-react';
import { ToolWindowHeader } from '../../components/ui/ToolWindowHeader';
import {
  cancelSelectionVoiceIntentPrompt,
  confirmSelectionVoiceIntentPrompt,
  getSelectionVoiceIntentPrompt,
  listenSelectionVoiceIntentShown,
} from '../../api/selection';
import { useModalActive } from '../../ui/layout/AppViewport';

export function SelectionVoiceIntentPicker() {
  const { t } = useTranslation();
  const modalActive = useModalActive();
  const [instruction, setInstruction] = useState('');
  const [sourceText, setSourceText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const generation = useRef(0);
  const acting = useRef(false);
  const promptReady = useRef(false);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    const load = async () => {
      const request = ++generation.current;
      acting.current = false;
      promptReady.current = false;
      setBusy(false);
      setReady(false);
      setError(null);
      setInstruction('');
      setSourceText('');
      try {
        const prompt = await getSelectionVoiceIntentPrompt();
        if (disposed || request !== generation.current || !prompt) return;
        setInstruction(prompt.instruction);
        setSourceText(prompt.sourceText);
        promptReady.current = true;
        setReady(true);
      } catch (reason) {
        if (!disposed && request === generation.current) setError(String(reason));
      }
    };
    void listenSelectionVoiceIntentShown(() => {
      if (!disposed) void load();
    })
      .then((remove) => {
        if (disposed) remove();
        else {
          stop = remove;
          void load();
        }
      })
      .catch((reason) => {
        if (!disposed) setError(String(reason));
      });
    return () => {
      disposed = true;
      generation.current += 1;
      acting.current = false;
      stop?.();
    };
  }, []);

  const invoke = async (action: () => Promise<void>) => {
    if (acting.current) return;
    acting.current = true;
    const request = generation.current;
    setBusy(true);
    setError(null);
    try {
      await action();
      // A successful choice hides the native prompt. Keep it inert until the next shown event.
    } catch (reason) {
      if (request === generation.current) {
        acting.current = false;
        setBusy(false);
        setError(String(reason));
      }
    }
  };
  const choose = (intent: 'question' | 'edit' | 'compose') => {
    if (promptReady.current) return invoke(() => confirmSelectionVoiceIntentPrompt(intent));
  };
  const cancel = useRef<() => void>();
  cancel.current = () => {
    void invoke(cancelSelectionVoiceIntentPrompt);
  };
  useEffect(() => {
    if (modalActive) return;
    const onKeydown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || event.defaultPrevented) return;
      event.preventDefault();
      cancel.current?.();
    };
    window.addEventListener('keydown', onKeydown);
    return () => window.removeEventListener('keydown', onKeydown);
  }, [modalActive]);

  return (
    <main className="ol-tool-window ol-intent-window">
      <ToolWindowHeader
        icon={<Sparkles />}
        title={t('selectionVoiceIntent.title')}
        description={t('selectionVoiceIntent.subtitle')}
        onClose={() => cancel.current?.()}
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
            disabled={busy || !ready}
            onClick={() => choose('question')}
          >
            <MessageCircleQuestion size={20} />
            {t('selectionVoiceIntent.question')}
          </button>
          {sourceText.trim() ? (
            <button
              className="ol-tool-button"
              disabled={busy || !ready}
              onClick={() => choose('edit')}
            >
              <PencilLine size={20} />
              {t('selectionVoiceIntent.edit')}
            </button>
          ) : null}
          <button
            className="ol-tool-button is-primary"
            disabled={busy || !ready}
            onClick={() => choose('compose')}
          >
            <PenLine size={20} />
            {t('selectionVoiceIntent.compose')}
          </button>
        </div>
        <button
          className="ol-tool-button ol-intent-cancel"
          disabled={busy}
          onClick={() => cancel.current?.()}
        >
          {t('selectionVoiceIntent.cancel')}
        </button>
      </footer>
    </main>
  );
}
