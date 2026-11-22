import { TextField } from '../../ui/controls/TextField';
import { memo, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession } from '../../api/contracts';
import { formatHistoryTime } from '../../lib/localeFormat';
import { useInitialListRows } from '../../lib/useInitialListRows';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { Pill } from '../../ui/controls/Pill';
import { formatDuration, historyTitle, TRUNCATED_PILL_STYLE } from './presentation';
import './history.css';

interface Props {
  searchInputRef: RefObject<HTMLInputElement>;
  searchShortcut: string;
  query: string;
  debouncedQuery: string;
  onQuery: (query: string) => void;
  filtered: DictationSession[];
  selectedId?: string;
  onSelect: (id: string) => void;
  trigger: string;
  loading: boolean;
  loadError: string | null;
  refresh: (notify: boolean) => Promise<void>;
  styleLabel: (session: DictationSession) => string;
}

export function HistoryList({
  searchInputRef,
  searchShortcut,
  query,
  debouncedQuery,
  onQuery,
  filtered,
  selectedId,
  onSelect,
  trigger,
  loading,
  loadError,
  refresh,
  styleLabel,
}: Props) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const rows = useInitialListRows(filtered);
  return (
    <Card padding={0} className="ol-history-list ol-col-4">
      <TextField
        ref={searchInputRef}
        className="ol-history-search"
        type="search"
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        placeholder={t('history.searchPlaceholder', { shortcut: searchShortcut })}
        aria-label={t('history.searchPlaceholder', { shortcut: searchShortcut })}
      />
      <div className="ol-thinscroll ol-history-list-scroll">
        <div className="ol-history-list-records">
          {loading && (
            <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>
              {t('common.loading')}
            </div>
          )}
          {!loading && loadError && (
            <div
              style={{
                padding: 16,
                fontSize: 12,
                color: 'var(--ol-ink-4)',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'flex-start',
                gap: 10,
              }}
            >
              <span>{t('history.loadFailed', { err: loadError })}</span>
              <Btn size="sm" variant="ghost" onClick={() => void refresh(true)}>
                {t('history.retry')}
              </Btn>
            </div>
          )}
          {!loading && !loadError && filtered.length === 0 && (
            <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>
              {debouncedQuery.trim()
                ? t('history.searchNoMatch', { query: debouncedQuery.trim() })
                : t('history.empty', { trigger })}
            </div>
          )}
          {rows.map((session) => (
            <HistoryRow
              key={session.id}
              session={session}
              selected={selectedId === session.id}
              onSelect={onSelect}
              styleLabel={styleLabel(session)}
              t={t}
              locale={locale}
            />
          ))}
        </div>
      </div>
    </Card>
  );
}

const HistoryRow = memo(function HistoryRow({
  session,
  selected,
  onSelect,
  styleLabel,
  t,
  locale,
}: {
  session: DictationSession;
  selected: boolean;
  onSelect: Props['onSelect'];
  styleLabel: string;
  t: ReturnType<typeof useTranslation>['t'];
  locale: string;
}) {
  return (
    <button
      type="button"
      data-history-record={session.id}
      aria-current={selected ? 'true' : undefined}
      onClick={() => onSelect(session.id)}
      className="ol-history-list-record"
      style={{
        width: '100%',
        padding: '10px 12px',
        textAlign: 'left',
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        border: '0.5px solid',
        borderColor: selected ? 'var(--ol-line)' : 'transparent',
        background: selected ? 'var(--ol-surface-2)' : 'transparent',
        cursor: 'default',
        fontFamily: 'inherit',
        transition:
          'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
        }}
      >
        <span
          style={{
            fontSize: 11,
            fontFamily: 'var(--ol-font-numeric)',
            fontVariantNumeric: 'tabular-nums',
            color: 'var(--ol-ink-3)',
          }}
        >
          {formatHistoryTime(session.createdAt, locale)}
        </span>
        <span
          style={{
            fontSize: 10,
            color: 'var(--ol-ink-4)',
            fontFamily: 'var(--ol-font-numeric)',
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          {formatDuration(session.durationMs, t, locale)}
        </span>
      </div>
      <div
        style={{
          fontSize: 12,
          color: 'var(--ol-ink-2)',
          lineHeight: 1.45,
          display: '-webkit-box',
          WebkitLineClamp: 2,
          WebkitBoxOrient: 'vertical',
          overflow: 'hidden',
        }}
      >
        {historyTitle(session, t)}
      </div>
      {/* Keep the base-mode color and use the style pack's name as the label. */}
      <div style={{ display: 'flex', minWidth: 0 }} title={styleLabel}>
        <Pill
          size="sm"
          tone={session.mode === 'raw' ? 'outline' : 'default'}
          style={TRUNCATED_PILL_STYLE}
        >
          {styleLabel}
        </Pill>
      </div>
    </button>
  );
});
