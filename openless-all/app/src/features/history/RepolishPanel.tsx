import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession, StylePack } from '../../api/contracts';
import { applyQuickNoteRepolish } from '../../api/history';
import { repolish } from '../../api/style-packs';
import { copyTextToClipboard } from '../../api/files';
import { AssistantMarkdown } from '../../components/chat/markdown';
import { Hint } from '../../components/Tooltip';
import { SelectLite } from '../../components/ui/SelectLite';
import { Button as Btn } from '../../ui/controls/Button';
import { Pill } from '../../ui/controls/Pill';
import {
  defaultPackId,
  packDisplayName,
  resolveRepolishRetryPackIdWithFallback,
} from '../../lib/history-repolish';
import { errorMessage, isTimeout, TRUNCATED_PILL_STYLE, useModeLabel } from './presentation';

interface RepolishResult {
  /** Key of the result card. Repeated applies of the same style overwrite the previous one
   *  instead of stacking cards forever. */
  key: string;
  title: string;
  text: string;
}

/**
 * Repolish panel: run the LLM again on this history entry's **raw text**.
 *
 * Both entries share one backend channel (`repolish`, stylePackId optional):
 * - "Retry with original style" → prefer the id of the pack that produced this entry (falling
 *   back to the currently active style when the pack is deleted / old history / not loaded).
 *   Rerunning with the same style is the control experiment that tells whether the previous
 *   result was model jitter or stable behavior — what the user really wants when they say
 *   "the AI recognized it wrong".
 * - "Apply" → pass the selected pack id, to see the same text in a different style.
 *
 * Results are shown only for this viewing session and are not written back to the history entry:
 * the entry's finalText is "what was actually inserted at the time", a factual record that a
 * later trial must not overwrite. The hint at the top of the panel says so directly.
 *
 * Note this reruns polish only, not recognition — history without an archived recording can only
 * use the raw text. The "retranscribe" entry point is open to all legacy ASR entries that still
 * have a recording.
 */
export function RepolishPanel({
  session,
  mobile,
  allPacks,
  packsError,
  onClose,
  persistOnApply,
}: {
  session: DictationSession;
  mobile: boolean;
  /** **All** style packs loaded at the History top level (including disabled); null = loading. */
  allPacks: StylePack[] | null;
  packsError: string | null;
  onClose: () => void;
  persistOnApply: boolean;
}) {
  const { t } = useTranslation();
  const MODE_LABEL = useModeLabel();
  const [selectedPackId, setSelectedPackId] = useState<string>('');
  const [running, setRunning] = useState<'retry' | 'apply' | null>(null);
  const [applyingKey, setApplyingKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [results, setResults] = useState<RepolishResult[]>([]);
  const canRun = session.rawTranscript.trim().length > 0;

  // List enabled packs only: disabled packs do not participate in polish elsewhere either;
  // listing them here would let "Apply" use a style the user believes is turned off.
  const packs = useMemo(() => (allPacks ? allPacks.filter((p) => p.enabled) : null), [allPacks]);

  useEffect(() => {
    if (!packs) return;
    setSelectedPackId((current) => current || defaultPackId(packs));
  }, [packs]);

  const run = async (kind: 'retry' | 'apply') => {
    // Retry prefers the original pack that produced this entry; if deleted / old history / not
    // loaded, fall back explicitly to the active pack (then the first available one) — keeping
    // the frontend label consistent with actual execution instead of letting the backend walk
    // its None fallback chain.
    const packId =
      kind === 'apply'
        ? selectedPackId
        : resolveRepolishRetryPackIdWithFallback(session, allPacks, packs ?? []);
    if (!canRun || (kind === 'apply' && !packId)) return;
    setRunning(kind);
    setError(null);
    try {
      const text = await repolish(session.rawTranscript, session.mode, packId);
      // Use allPacks rather than packs to look up the name: retrying with a disabled original
      // pack still shows its real name in the title.
      const pack = packId ? allPacks?.find((p) => p.id === packId) : undefined;
      const result: RepolishResult = {
        key: packId ?? '__retry__',
        title: pack
          ? t('history.repolish.resultTitle', { name: packDisplayName(pack, MODE_LABEL) })
          : t('history.repolish.retryResultTitle'),
        text,
      };
      // The same key overwrites the old result; a new key is prepended — the latest trial stays
      // closest to the action area.
      setResults((prev) => [result, ...prev.filter((r) => r.key !== result.key)]);
    } catch (err) {
      console.error('[history] repolish failed', err);
      const msg = errorMessage(err);
      // The backend passes LLMError::Timeout through as the bare string "timeout"; showing it
      // directly tells the user nothing — "Repolish failed: timeout" reads like the feature is
      // broken when the actual cause is the current LLM provider not responding within 30s
      // (especially common in the free model pool). Use a message the user can act on.
      setError(
        isTimeout(msg) ? t('history.repolish.timeout') : t('history.repolish.failed', { err: msg }),
      );
    } finally {
      setRunning(null);
    }
  };

  const applyResult = async (result: RepolishResult) => {
    if (!persistOnApply) return;
    setApplyingKey(result.key);
    setError(null);
    try {
      await applyQuickNoteRepolish(
        session.id,
        result.text,
        result.key === '__retry__' ? (session.stylePackId ?? undefined) : result.key,
      );
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setApplyingKey(null);
    }
  };

  return (
    <div style={{ marginTop: 18, paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 10,
          flexWrap: 'wrap',
          marginBottom: 12,
        }}
      >
        <span
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            fontSize: 12,
            fontWeight: 'var(--ol-font-weight-semibold)',
            color: 'var(--ol-ink-2)',
          }}
        >
          <Hint content={t('history.repolish.hint')}>{t('history.repolish.title')}</Hint>
        </span>
        <span style={{ display: 'inline-flex', gap: 6 }}>
          {results.length > 0 && (
            <Btn size="sm" variant="ghost" onClick={() => setResults([])}>
              {t('history.repolish.clear')}
            </Btn>
          )}
          <Btn size="sm" variant="ghost" onClick={onClose}>
            {t('common.close')}
          </Btn>
        </span>
      </div>

      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          flexWrap: 'wrap',
          marginBottom: results.length > 0 ? 14 : 0,
        }}
      >
        {!canRun && (
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginBottom: 10 }}>
            {t('quickNote.repolishNeedsTranscript', 'Re-transcribe the audio before repolishing.')}
          </div>
        )}
        <Btn
          icon="refresh"
          variant="ghost"
          size="sm"
          disabled={running !== null || !canRun}
          onClick={() => void run('retry')}
        >
          {running === 'retry' ? t('history.repolish.retrying') : t('history.repolish.retry')}
        </Btn>
        <span style={{ width: 1, height: 18, background: 'var(--ol-line)' }} />
        {packsError ? (
          <span style={{ fontSize: 11, color: 'var(--ol-red, #ef4444)' }}>
            {t('history.repolish.packsLoadFailed', { err: packsError })}
          </span>
        ) : packs && packs.length === 0 ? (
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
            {t('history.repolish.noPacks')}
          </span>
        ) : (
          <>
            {/* Use the official SelectLite consistently; no more mixing in a native <select>. */}
            <SelectLite
              value={selectedPackId}
              onChange={setSelectedPackId}
              ariaLabel={t('history.repolish.pickStyle')}
              disabled={!packs || running !== null}
              options={(packs ?? []).map((pack) => ({
                value: pack.id,
                label: packDisplayName(pack, MODE_LABEL),
              }))}
              style={{ maxWidth: mobile ? 160 : 220, minWidth: 0, height: 30, fontSize: 13 }}
            />
            <Btn
              variant="ghost"
              size="sm"
              disabled={!selectedPackId || running !== null || !canRun}
              onClick={() => void run('apply')}
            >
              {running === 'apply' ? t('history.repolish.applying') : t('history.repolish.apply')}
            </Btn>
          </>
        )}
      </div>

      {error && (
        <div
          style={{
            marginTop: 10,
            padding: '8px 10px',
            borderRadius: 'var(--ol-control-radius)',
            background: 'rgba(239,68,68,0.08)',
            color: 'var(--ol-red, #ef4444)',
            fontSize: 11.5,
            lineHeight: 1.45,
          }}
        >
          {error}
        </div>
      )}

      {results.length > 0 && (
        <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1fr', gap: 12 }}>
          {results.map((result) => (
            <HistoryResultCard
              key={result.key}
              title={result.title}
              text={result.text}
              applyLabel={persistOnApply ? t('quickNote.applyResult', 'Apply to note') : undefined}
              applying={applyingKey === result.key}
              onApply={persistOnApply ? () => void applyResult(result) : undefined}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function HistoryResultCard({
  title,
  text,
  applyLabel,
  applying = false,
  onApply,
}: {
  title: string;
  text: string;
  applyLabel?: string;
  applying?: boolean;
  onApply?: () => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const onCopy = async () => {
    try {
      await copyTextToClipboard(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy result', error);
    }
  };

  return (
    // minWidth: 0 — grid children default to min-width: auto; a non-wrapping title Pill would
    // push the card outside the result grid (same class of problem as the detail page's two
    // column text cards).
    <div
      style={{
        minWidth: 0,
        padding: 14,
        border: '0.5px dashed var(--ol-line-strong)',
        borderRadius: 'var(--ol-r-md)',
        background: 'var(--ol-surface-2)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
          marginBottom: 10,
        }}
      >
        <span style={{ display: 'flex', minWidth: 0 }} title={title}>
          <Pill size="sm" tone="default" style={TRUNCATED_PILL_STYLE}>
            {title}
          </Pill>
        </span>
        {text.trim() && (
          <Btn
            icon={copied ? 'check' : 'copy'}
            variant="ghost"
            size="sm"
            onClick={() => void onCopy()}
          >
            {copied ? t('common.copied') : t('common.copy')}
          </Btn>
        )}
        {onApply && text.trim() && (
          <Btn variant="ghost" size="sm" disabled={applying} onClick={onApply}>
            {applying ? t('quickNote.applying', 'Applying…') : applyLabel}
          </Btn>
        )}
      </div>
      <div style={{ fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)' }}>
        <AssistantMarkdown markdown={text.trim() || t('history.repolish.empty')} />
      </div>
    </div>
  );
}
