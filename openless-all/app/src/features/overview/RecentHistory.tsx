import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession, PolishMode } from '../../api/contracts';
import { copyTextToClipboard } from '../../api/files';
import { formatHistoryTime, formatLocaleDecimal } from '../../lib/localeFormat';
import { Button } from '../../ui/controls/Button';
import { Pill } from '../../ui/controls/Pill';
import { Card } from '../../ui/layout/Card';

function useModeLabels(): Record<PolishMode, string> {
  const { t } = useTranslation();
  return {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
}

export function RecentHistory({
  history,
  historyError,
  onOpenHistory,
  onRetry,
  loading,
}: {
  history: DictationSession[];
  historyError: boolean;
  loading: boolean;
  onOpenHistory?: () => void;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  const modeLabel = useModeLabels();
  return (
    <Card
      padding={0}
      className="ol-overview-recent-card ol-col-7"
      style={{
        display: 'flex',
        flexDirection: 'column',
        minWidth: 0,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          padding: '12px var(--ol-grid-gutter)',
          borderBottom: '0.5px solid var(--ol-line)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          flexShrink: 0,
        }}
      >
        <span
          style={{
            fontSize: 13,
            fontWeight: 'var(--ol-font-weight-semibold)',
            color: 'var(--ol-ink-2)',
          }}
        >
          {t('overview.recentTitle')}
        </span>
        <Button size="sm" variant="ghost" disabled={!onOpenHistory} onClick={onOpenHistory}>
          {t('overview.recentAll')}
        </Button>
      </div>
      <div
        className="ol-thinscroll ol-overview-recent"
        tabIndex={0}
        style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}
      >
        {historyError ? (
          <div className="ol-overview-recent-error">
            <span>{t('overview.recentLoadFailed')}</span>
            <Button size="sm" variant="ghost" onClick={onRetry}>
              {t('overview.historyRetry')}
            </Button>
          </div>
        ) : (
          <>
            {(loading || history.length === 0) && (
              <div
                style={{
                  padding: 24,
                  textAlign: 'center',
                  fontSize: 12,
                  color: 'var(--ol-ink-4)',
                }}
              >
                {t(loading ? 'common.loading' : 'overview.recentEmptyHint')}
              </div>
            )}
            {history.map((s) => (
              <RecentRow key={s.id} session={s} modeLabel={modeLabel} />
            ))}
          </>
        )}
      </div>
    </Card>
  );
}

function RecentRow({
  session,
  modeLabel,
}: {
  session: DictationSession;
  modeLabel: Record<PolishMode, string>;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const [copied, setCopied] = useState(false);
  const timer = useRef<number>();
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      window.clearTimeout(timer.current);
    };
  }, []);

  const onCopy = async () => {
    try {
      // Same as History: when polish failed / produced nothing, finalText is empty and the copy falls back to
      // the raw transcript, avoiding copying an empty string.
      await copyTextToClipboard(
        session.finalText.trim() ? session.finalText : session.rawTranscript,
      );
      if (!mounted.current) return;
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('[overview] failed to copy recent entry', error);
    }
  };

  return (
    <div
      data-overview-record={session.id}
      style={{
        padding: '12px 18px',
        borderBottom: '0.5px solid var(--ol-line-soft)',
        display: 'flex',
        gap: 12,
        alignItems: 'flex-start',
      }}
    >
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'flex-start',
          gap: 4,
          minWidth: 60,
        }}
      >
        <span
          style={{
            fontSize: 12.5,
            fontFamily: 'var(--ol-font-mono)',
            color: 'var(--ol-ink-3)',
          }}
        >
          {formatHistoryTime(session.createdAt, locale)}
        </span>
        <Pill size="sm" tone="default">
          {modeLabel[session.mode]}
        </Pill>
      </div>
      <div
        style={{
          flex: 1,
          fontSize: 14,
          color: 'var(--ol-ink-2)',
          whiteSpace: 'pre-line',
          lineHeight: 1.55,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          display: '-webkit-box',
          WebkitLineClamp: 2,
          WebkitBoxOrient: 'vertical',
        }}
      >
        {session.finalText.split('\n')[0]}
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 6 }}>
        <span style={{ fontSize: 12, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
          {formatDuration(session.durationMs ?? 0, t, locale)}
        </span>
        <Button
          size="sm"
          variant="ghost"
          icon={copied ? 'check' : 'copy'}
          onClick={() => void onCopy()}
          style={{ padding: '3px 8px' }}
        >
          {copied ? t('common.copied') : t('common.copy')}
        </Button>
      </div>
    </div>
  );
}

function formatDuration(
  ms: number,
  t: ReturnType<typeof useTranslation>['t'],
  locale: string,
): string {
  if (ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: formatLocaleDecimal(sec, locale) });
  return `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`;
}
