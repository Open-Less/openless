import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  acceptHistoryLearning,
  beginHistoryLearning,
  cancelHistoryLearning,
  submitHistoryLearning,
  type HistoryLearningErrorKey,
} from '../../api/history';
import type { DialogActions } from '../../api/interaction';
import { DialogAction } from '../../ui/overlays/Dialog';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { VocabCorrectionRow } from '../vocabulary-learning/VocabSuggestionCard';

type Source = Awaited<ReturnType<typeof beginHistoryLearning>>;
type LearningResult = Awaited<ReturnType<typeof submitHistoryLearning>>;
type LearningOptions = { vocabulary: boolean; expression: boolean };

const errorLabels = {
  learningNotEligible: 'historyLearning.unavailable',
  learningSourceExpired: 'historyLearning.sourceChanged',
  learningPermissionOff: 'historyLearning.disabled',
  learningPayloadConflict: 'historyLearning.sourceChanged',
  learningEmpty: 'historyLearning.unavailable',
  learningLimit: 'historyLearning.limit',
  learningStoreFailed: 'common.error',
  learningWindowDenied: 'historyLearning.unavailable',
} as const satisfies Record<HistoryLearningErrorKey, string>;

function learningError(error: unknown): keyof typeof errorLabels | undefined {
  const fields = typeof error === 'object' && error !== null
    ? error as Record<string, unknown> : {};
  return [error, fields.code, fields.message, fields.reason].find((value): value is keyof typeof errorLabels =>
    typeof value === 'string' && Object.hasOwn(errorLabels, value),
  );
}

function releaseSource(token: string) {
  void cancelHistoryLearning(token).catch(() => {});
}

export function useHistoryLearning(historyId: string, options: LearningOptions) {
  const { t } = useTranslation();
  const { openDialog } = useInteraction(`history-learning:${historyId}`);
  return async function openHistoryLearning() {
    const request = new AbortController();
    try {
      return await openDialog<boolean>({
        title: t('historyLearning.title'),
        width: 'min(520px, 100%)',
        render: (actions) => (
          <HistoryLearningEditor
            historyId={historyId}
            options={options}
            signal={request.signal}
            actions={actions}
          />
        ),
      });
    } finally {
      request.abort();
    }
  };
}

function HistoryLearningEditor({
  historyId,
  options,
  signal,
  actions,
}: {
  historyId: string;
  options: LearningOptions;
  signal: AbortSignal;
  actions: DialogActions<boolean>;
}) {
  const { t } = useTranslation();
  const originalId = useId();
  const correctionId = useId();
  const reviewFocus = useRef<HTMLDivElement>(null);
  const correctedInput = useRef<HTMLTextAreaElement>(null);
  const source = useRef<Source>();
  const operation = useRef(false);
  const [original, setOriginal] = useState<string>();
  const [corrected, setCorrected] = useState('');
  const [vocabulary, setVocabulary] = useState(options.vocabulary);
  const [expression, setExpression] = useState(false);
  const [busy, setBusy] = useState(false);
  const [locked, setLocked] = useState(false);
  const [failure, setFailure] = useState<string>();
  const [result, setResult] = useState<LearningResult>();
  const [dismissed, setDismissed] = useState<string[]>([]);
  const [wordSaved, setWordSaved] = useState(false);

  useEffect(() => {
    if (signal.aborted) return;
    let disposed = false;
    let timer: number | undefined;
    function dispose() {
      disposed = true;
      window.clearTimeout(timer);
      if (source.current) releaseSource(source.current.token);
      source.current = undefined;
    }
    function close() {
      dispose();
      setOriginal(undefined);
      setCorrected('');
      setResult(undefined);
    }
    signal.addEventListener('abort', close, { once: true });
    void beginHistoryLearning(historyId).then((opened) => {
      if (disposed || opened.historyId !== historyId) {
        releaseSource(opened.token);
        if (!disposed) setFailure('historyLearning.sourceChanged');
        return;
      }
      if (opened.expiresAtMs <= Date.now()) {
        releaseSource(opened.token);
        setFailure('historyLearning.expired');
        return;
      }
      source.current = opened;
      timer = window.setTimeout(() => {
        invalidate('historyLearning.expired');
      }, Math.max(0, opened.expiresAtMs - Date.now()));
      setOriginal(opened.originalText);
      setCorrected(opened.originalText);
    }).catch((error: unknown) => {
      if (disposed) return;
      const key = learningError(error);
      setFailure(key ? errorLabels[key] : 'common.error');
    });
    return () => {
      signal.removeEventListener('abort', close);
      dispose();
    };
  }, [historyId, signal]);

  function invalidate(message: string) {
    if (source.current) releaseSource(source.current.token);
    source.current = undefined;
    setOriginal(undefined);
    setCorrected('');
    setResult(undefined);
    setFailure(message);
    setBusy(false);
  }

  useEffect(() => {
    if (original !== undefined && !signal.aborted) correctedInput.current?.focus();
  }, [original, signal]);

  async function operate(run: (token: string, isCurrent: () => boolean) => Promise<void>) {
    const opened = source.current;
    if (!opened || signal.aborted || operation.current) return;
    if (opened.expiresAtMs <= Date.now()) {
      invalidate('historyLearning.expired');
      return;
    }
    const isCurrent = () => !signal.aborted && source.current === opened;
    operation.current = true;
    setBusy(true);
    setFailure(undefined);
    try {
      await run(opened.token, isCurrent);
    } catch (error) {
      if (!isCurrent()) return;
      const key = learningError(error);
      if (!key || key === 'learningStoreFailed') {
        setFailure('common.error');
      } else {
        invalidate(errorLabels[key]);
      }
    } finally {
      operation.current = false;
      if (isCurrent()) setBusy(false);
    }
  }

  function review() {
    if (!corrected.trim() || corrected === original || (!vocabulary && !expression)) return;
    void operate(async (token, isCurrent) => {
      // A retry must submit the same payload once the backend may have received it.
      setLocked(true);
      const next = await submitHistoryLearning(token, corrected, vocabulary, expression);
      if (!isCurrent()) return;
      setResult(next);
      setOriginal(undefined);
      setCorrected('');
    });
  }

  function dismissWord(candidateId: string) {
    setDismissed((current) => [...current, candidateId]);
    reviewFocus.current?.focus({ preventScroll: true });
  }

  function acceptWord(candidateId: string) {
    void operate(async (token, isCurrent) => {
      await acceptHistoryLearning(token, candidateId);
      if (!isCurrent()) return;
      setWordSaved(true);
      dismissWord(candidateId);
    });
  }

  useEffect(() => {
    if (result) reviewFocus.current?.focus({ preventScroll: true });
  }, [result]);

  if (signal.aborted) return null;
  const editing = original !== undefined;
  const tooLong = corrected.length > 32768 || [...corrected].length > 16384;
  return (
    <div className="ol-history-learning">
      {!editing && !result && !failure && <div role="status">{t('common.loading')}</div>}
      {editing && (
        <>
          <div className="ol-history-learning-field">
            <span id={originalId}>{t('historyLearning.original')}</span>
            <pre className="ol-history-learning-original" tabIndex={0} aria-labelledby={originalId}>
              {original}
            </pre>
          </div>
          <div className="ol-history-learning-field">
            <label htmlFor={correctionId}>{t('historyLearning.corrected')}</label>
            <textarea
              ref={correctedInput}
              id={correctionId}
              rows={4}
              value={corrected}
              readOnly={locked}
              aria-invalid={tooLong || undefined}
              onChange={(event) => setCorrected(event.currentTarget.value)}
            />
          </div>
          {options.vocabulary && (
            <label className="ol-dialog-acknowledgement">
              <input type="checkbox" checked={vocabulary} disabled={locked} onChange={(event) => setVocabulary(event.currentTarget.checked)} />
              {t('historyLearning.vocabulary')}
            </label>
          )}
          {options.expression && (
            <label className="ol-dialog-acknowledgement">
              <input type="checkbox" checked={expression} disabled={locked} onChange={(event) => setExpression(event.currentTarget.checked)} />
              {t('historyLearning.expression')}
            </label>
          )}
        </>
      )}
      {result && (
        <section className="ol-history-learning-review" aria-labelledby={originalId}>
          <div id={originalId} ref={reviewFocus} tabIndex={-1}>{t('historyLearning.source')}</div>
          <div role="status">
            {result.expressionRecorded && <div>{t('historyLearning.expressionRecorded')}</div>}
            {wordSaved && <div>{t('historyLearning.vocabularySaved')}</div>}
            {!wordSaved && !result.expressionRecorded && !result.candidates.length &&
              t('historyLearning.noLearning')}
          </div>
          {result.candidates.filter((candidate) => !dismissed.includes(candidate.id)).map((candidate) => (
            <VocabCorrectionRow
              key={candidate.id}
              inline
              suggestion={candidate}
              disabled={busy}
              onAccept={() => acceptWord(candidate.id)}
              onReject={() => dismissWord(candidate.id)}
            />
          ))}
        </section>
      )}
      {failure && <div className="ol-history-learning-error" role="alert">{t(failure)}</div>}
      {tooLong && <div className="ol-history-learning-error" role="alert">{t('historyLearning.limit')}</div>}
      <div className="ol-history-learning-actions">
        <DialogAction tone={editing || !result ? 'neutral' : 'primary'} onClick={() =>
          result ? actions.resolve(result.expressionRecorded || wordSaved) : actions.cancel()
        }>
          {result || (!editing && failure) ? t('common.close') : t('common.cancel')}
        </DialogAction>
        {editing && (
          <DialogAction
            disabled={busy || tooLong || !corrected.trim() || corrected === original || (!vocabulary && !expression)}
            onClick={review}
          >
            {busy ? t('common.loading') : locked ? t('historyLearning.retry') : t('historyLearning.review')}
          </DialogAction>
        )}
      </div>
    </div>
  );
}
