import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Mic, RotateCcw, Square, X } from 'lucide-react';
import {
  cancelVoiceEditSession,
  commitVoiceEditSession,
  finalizeVoiceEditDictation,
  finalizeVoiceEditInstruction,
  getVoiceEditState,
  startVoiceEditInstruction,
  startVoiceEditSession,
  type VoiceEditSnapshot,
} from '../lib/ipc';
import { cancelAndCloseVoiceEditSession } from '../lib/ipc/voice-edit-session';
import { ToolWindowHeader } from '../components/ui/ToolWindowHeader';
import './voice-edit-panel.css';

const isTerminal = (phase: VoiceEditSnapshot['phase']) =>
  phase === 'completed' || phase === 'cancelled';
const errorDetail = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason);

interface VoiceEditPanelProps {
  embedded?: boolean;
  active?: boolean;
  closeError?: string;
  onRequestClose?: () => void;
}

export function VoiceEditPanel({
  embedded = false,
  active = true,
  closeError = '',
  onRequestClose,
}: VoiceEditPanelProps) {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<VoiceEditSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [closing, setClosing] = useState(false);
  const [error, setError] = useState('');
  const generation = useRef(0);
  const closingRef = useRef(false);

  const refresh = useCallback(async () => {
    if (closingRef.current) return;
    const request = generation.current;
    try {
      const next = await getVoiceEditState();
      if (request === generation.current) setSnapshot(next);
    } catch (reason) {
      if (request === generation.current) setError(errorDetail(reason));
    }
  }, []);

  useEffect(() => {
    setBusy(false);
    if (!active) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 500);
    return () => {
      generation.current += 1;
      window.clearInterval(timer);
    };
  }, [active, refresh]);

  useEffect(() => {
    if (closeError) setError(closeError);
  }, [closeError]);

  const run = async (action: () => Promise<VoiceEditSnapshot | null>) => {
    const request = ++generation.current;
    setBusy(true);
    setError('');
    try {
      const next = await action();
      if (request !== generation.current) return;
      if (next) setSnapshot(next);
      else await refresh();
    } catch (reason) {
      if (request === generation.current) {
        setError(errorDetail(reason));
        await refresh();
      }
    } finally {
      if (request === generation.current) setBusy(false);
    }
  };

  const runSession = (action: (sessionId: string) => Promise<VoiceEditSnapshot | null>) => {
    if (snapshot) return run(() => action(snapshot.sessionId));
  };

  const phase = snapshot?.phase;
  const context = snapshot?.context;
  const canCancel = Boolean(snapshot && !isTerminal(snapshot.phase) && !closing);
  const closePanel = useCallback(async () => {
    if (closingRef.current) return false;
    closingRef.current = true;
    generation.current += 1;
    setClosing(true);
    setError('');
    try {
      await cancelAndCloseVoiceEditSession(
        snapshot && !isTerminal(snapshot.phase) ? snapshot.sessionId : undefined,
      );
      onRequestClose?.();
      return true;
    } catch (reason) {
      setError(errorDetail(reason));
      return false;
    } finally {
      closingRef.current = false;
      setClosing(false);
      setBusy(false);
    }
  }, [onRequestClose, snapshot]);

  useEffect(() => {
    if (!embedded || !active) return;
    const onPopState = () => {
      void closePanel().then((closed) => {
        if (!closed) {
          window.history.pushState({ openlessVoiceEdit: true }, '', window.location.href);
        }
      });
    };
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, [active, closePanel, embedded]);

  return (
    <main className="ol-tool-window ol-voice-edit-window">
      <ToolWindowHeader
        icon={<Mic />}
        title={t('voiceEdit.title')}
        description={t('voiceEdit.description')}
        onClose={() => {
          void closePanel();
        }}
        closeLabel={t('common.close')}
        closeDisabled={closing}
      />
      <section className={`ol-voice-edit-content${embedded ? ' is-embedded' : ''}`}>
        {!snapshot || isTerminal(snapshot.phase) ? (
          <div className="ol-voice-edit-empty">
            <p>{t(embedded ? 'voiceEdit.androidTargetHint' : 'voiceEdit.targetHint')}</p>
            {!embedded && (
              <button
                className="ol-tool-button is-primary"
                disabled={busy || closing}
                onClick={() => void run(() => startVoiceEditSession())}
              >
                <Mic size={18} />
                {t('voiceEdit.startDraft')}
              </button>
            )}
          </div>
        ) : (
          <>
            <div className="ol-voice-edit-status">
              <span aria-live="polite">
                {t('voiceEdit.status', { phase: t(`voiceEdit.phase.${snapshot.phase}`) })}
              </span>
              <span>{t('voiceEdit.turns', { count: context?.turns.length ?? 0 })}</span>
            </div>
            {context?.draft ? (
              <div className="ol-voice-edit-card">
                <span className="ol-voice-edit-label">{t('voiceEdit.draft')}</span>
                <div>{context.draft}</div>
              </div>
            ) : null}
            {context?.preview && context.preview !== context.draft ? (
              <div className="ol-voice-edit-card is-preview">
                <span className="ol-voice-edit-label">{t('voiceEdit.preview')}</span>
                <div>{context.preview}</div>
              </div>
            ) : null}
            {context?.turns.length ? (
              <div className="ol-voice-edit-history">
                <span className="ol-voice-edit-label">{t('voiceEdit.history')}</span>
                {context.turns.map((turn, index) => (
                  <div className="ol-voice-edit-turn" key={`${snapshot.sessionId}-${index}`}>
                    <div>
                      <b>{t('voiceEdit.turn', { count: index + 1 })}</b>: {turn.instructionRaw}
                    </div>
                    {turn.instructionPolished !== turn.instructionRaw ? (
                      <div className="ol-voice-edit-turn-polished">
                        {t('voiceEdit.polishedInstruction', { text: turn.instructionPolished })}
                      </div>
                    ) : null}
                    <div className="ol-voice-edit-turn-preview">
                      {t('voiceEdit.turnPreview', { text: turn.previewAfter })}
                    </div>
                  </div>
                ))}
              </div>
            ) : null}
            <div className="ol-voice-edit-actions">
              {phase === 'dictating' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy || closing}
                  onClick={() => void runSession(finalizeVoiceEditDictation)}
                >
                  <Square size={16} />
                  {t('voiceEdit.finishDraft')}
                </button>
              ) : null}
              {phase === 'draft_ready' || phase === 'preview' ? (
                <button
                  className="ol-tool-button"
                  disabled={busy || closing}
                  onClick={() => void runSession(startVoiceEditInstruction)}
                >
                  <Mic size={16} />
                  {t('voiceEdit.nextInstruction')}
                </button>
              ) : null}
              {phase === 'editing' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy || closing}
                  onClick={() => void runSession(finalizeVoiceEditInstruction)}
                >
                  <Square size={16} />
                  {t('voiceEdit.finishInstruction')}
                </button>
              ) : null}
              {phase === 'draft_ready' || phase === 'preview' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy || closing}
                  onClick={() => void runSession(commitVoiceEditSession)}
                >
                  <Check size={16} />
                  {t('voiceEdit.commit')}
                </button>
              ) : null}
              {canCancel ? (
                <button
                  className="ol-tool-button ol-voice-edit-cancel"
                  disabled={closing}
                  onClick={() => void runSession(cancelVoiceEditSession)}
                >
                  <X size={16} />
                  {t('common.cancel')}
                </button>
              ) : null}
            </div>
          </>
        )}
        {error ? (
          <div className="ol-tool-error" role="alert">
            {t(`voiceEdit.errors.${error.split(':')[0]}`, { defaultValue: error })}
          </div>
        ) : null}
        {!embedded && snapshot && isTerminal(snapshot.phase) ? (
          <button
            className="ol-tool-button"
            disabled={busy || closing}
            onClick={() => void run(() => startVoiceEditSession())}
          >
            <RotateCcw size={16} />
            {t('voiceEdit.restart')}
          </button>
        ) : null}
      </section>
    </main>
  );
}
