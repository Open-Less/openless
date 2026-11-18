import type { RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession } from '../../api/contracts';
import { Icon } from '../../components/Icon';
import { formatHistoryTime } from '../../lib/localeFormat';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { Pill } from '../../ui/controls/Pill';
import { formatDuration, historyTitle, TRUNCATED_PILL_STYLE } from './presentation';

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
  return (
    <Card
      padding={0}
      className="ol-history-list ol-col-4"
      style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}
    >
      {/* The left column is one scroll container; the search box sticks to the top with an
              opaque background — the list scrolls underneath it. Style filter chips and the
              "N entries" count row are dropped; only search remains. */}
      <div className="ol-thinscroll" style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
        <div
          style={{
            position: 'sticky',
            top: 0,
            zIndex: 2,
            background: 'var(--ol-surface-raised)',
            padding: '12px 14px',
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              padding: '6px 10px',
              fontSize: 12,
              border: '0.5px solid var(--ol-line-strong)',
              borderRadius: 'var(--ol-control-radius)',
              background: 'var(--ol-surface-2)',
              color: 'var(--ol-ink-3)',
            }}
          >
            <Icon name="search" size={12} />
            <input
              ref={searchInputRef}
              type="search"
              value={query}
              onChange={(e) => onQuery(e.target.value)}
              placeholder={t('history.searchPlaceholder', { shortcut: searchShortcut })}
              aria-label={t('history.searchPlaceholder', { shortcut: searchShortcut })}
              style={{
                flex: 1,
                minWidth: 0,
                outline: 'none',
                border: 0,
                background: 'transparent',
                fontSize: 12,
                color: 'var(--ol-ink-1)',
                fontFamily: 'inherit',
              }}
            />
          </div>
        </div>
        <div style={{ padding: 6 }}>
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
          {filtered.map((s) => (
            <button
              key={s.id}
              type="button"
              data-history-record={s.id}
              aria-current={selectedId === s.id ? 'true' : undefined}
              onClick={() => onSelect(s.id)}
              style={{
                width: '100%',
                padding: '10px 12px',
                textAlign: 'left',
                display: 'flex',
                flexDirection: 'column',
                gap: 4,
                border: '0.5px solid',
                borderColor: selectedId === s.id ? 'var(--ol-line)' : 'transparent',
                borderRadius: 'var(--ol-r-md)',
                background: selectedId === s.id ? 'var(--ol-surface-2)' : 'transparent',
                cursor: 'default',
                fontFamily: 'inherit',
                marginBottom: 4,
                transition:
                  'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
                // Rows fade in and drop slightly when search filters or new records
                // insert.
                animation: 'ol-item-in 0.22s var(--ol-motion-spring) both',
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
                  {formatHistoryTime(s.createdAt, locale)}
                </span>
                <span
                  style={{
                    fontSize: 10,
                    color: 'var(--ol-ink-4)',
                    fontFamily: 'var(--ol-font-numeric)',
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {formatDuration(s.durationMs, t, locale)}
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
                {historyTitle(s, t)}
              </div>
              {/* tone still follows baseMode: the color keeps the original coarse
                        category info, the text becomes the actual style pack name. */}
              <div style={{ display: 'flex', minWidth: 0 }} title={styleLabel(s)}>
                <Pill
                  size="sm"
                  tone={s.mode === 'raw' ? 'outline' : 'default'}
                  style={TRUNCATED_PILL_STYLE}
                >
                  {styleLabel(s)}
                </Pill>
              </div>
            </button>
          ))}
        </div>
      </div>
    </Card>
  );
}
