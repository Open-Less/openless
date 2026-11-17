import { useTranslation } from 'react-i18next';
import {
  ACTIVITY_METRICS,
  ACTIVITY_PERIODS,
  buildPeriodSeries,
  type ActivityMetric,
  type ActivityPeriod,
} from '../../lib/activityMetrics';
import { formatLocaleDate, formatLocaleNumber } from '../../lib/localeFormat';
import { Btn, Card } from '../../pages/_atoms';

/** Segmented toggle (shared by period / metric). Narrow enough to fit both groups on one line. */
function SegmentedToggle<T extends string | number>({
  value,
  options,
  onChange,
  ariaLabel,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (next: T) => void;
  ariaLabel: string;
}) {
  return (
    <div
      className="ol-overview-segment"
      role="group"
      aria-label={ariaLabel}
      style={{
        display: 'flex',
        gap: 2,
        padding: 2,
        borderRadius: 'var(--ol-control-radius)',
        background: 'var(--ol-surface-2)',
        boxShadow: 'inset 0 0 0 0.5px var(--ol-line)',
      }}
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={String(option.value)}
            type="button"
            aria-pressed={selected}
            onClick={() => onChange(option.value)}
            style={{
              padding: '3px 9px',
              fontSize: 12.5,
              fontWeight: selected
                ? 'var(--ol-font-weight-semibold)'
                : 'var(--ol-font-weight-medium)',
              border: 0,
              borderRadius: 'var(--ol-r-sm)',
              background: selected ? 'var(--ol-blue)' : 'transparent',
              color: selected ? '#fff' : 'var(--ol-ink-3)',
              cursor: 'pointer',
              fontFamily: 'inherit',
              whiteSpace: 'nowrap',
              transition:
                'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
            }}
          >
            <span className="ol-segmented-label" data-label={option.label}>
              {option.label}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/**
 * Period metrics card: last 7 / 30 days × count / chars / duration.
 *
 * The card top shows the period total (large) + daily average, chart below — the user's question
 * "how many characters did I dictate this month" is one number, not summing 30 bars by eye.
 */
export function PeriodMetricsCard({
  series,
  period,
  metric,
  onPeriodChange,
  onMetricChange,
  loadError,
  onRetry,
}: {
  series: ReturnType<typeof buildPeriodSeries>;
  period: ActivityPeriod;
  metric: ActivityMetric;
  onPeriodChange: (next: ActivityPeriod) => void;
  onMetricChange: (next: ActivityMetric) => void;
  loadError: boolean;
  onRetry: () => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const periodOptions = ACTIVITY_PERIODS.map((days) => ({
    value: days,
    label: t(`overview.period.last${days}Days`),
  }));
  const metricOptions = ACTIVITY_METRICS.map((id) => ({
    value: id,
    label: t(`overview.metricName.${id}`),
  }));

  return (
    <Card
      className="ol-overview-period"
      padding={18}
      style={{
        display: 'flex',
        flexDirection: 'column',
        minWidth: 0,
        minHeight: 240,
      }}
    >
      {/* flexWrap: the card is narrow inside a 1fr column; wrap instead of squashing buttons when the two toggles don't fit. */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
          flexWrap: 'wrap',
          marginBottom: 12,
          flexShrink: 0,
        }}
      >
        <SegmentedToggle
          value={period}
          options={periodOptions}
          onChange={onPeriodChange}
          ariaLabel={t('overview.period.ariaLabel')}
        />
        <SegmentedToggle
          value={metric}
          options={metricOptions}
          onChange={onMetricChange}
          ariaLabel={t('overview.metricName.ariaLabel')}
        />
      </div>

      {loadError ? (
        <div
          style={{
            minHeight: 132,
            display: 'flex',
            flexDirection: 'column',
            gap: 10,
            alignItems: 'center',
            justifyContent: 'center',
            textAlign: 'center',
            fontSize: 12,
            color: 'var(--ol-ink-4)',
          }}
        >
          {t('overview.activityLoadError')}
          <Btn size="sm" onClick={onRetry}>
            {t('overview.historyRetry')}
          </Btn>
        </div>
      ) : (
        <>
          <div style={{ marginBottom: 12, flexShrink: 0 }}>
            <div
              style={{
                fontSize: 26,
                fontWeight: 'var(--ol-font-weight-semibold)',
                letterSpacing: '-0.02em',
                color: 'var(--ol-ink)',
                lineHeight: 1.1,
              }}
            >
              {formatMetricValue(series.total, metric, t, locale)}
            </div>
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)', marginTop: 5 }}>
              {t('overview.period.dailyAverage', {
                value: formatMetricValue(series.dailyAverage, metric, t, locale),
              })}
            </div>
          </div>
          <PeriodChart series={series} metric={metric} />
        </>
      )}
    </Card>
  );
}

/** Bar chart. With 7 days each bar is labeled; at 30 days bars are a few pixels wide and labels would blur,
 *  so values move to hover titles and dates are labeled only at the ends and middle. */
function PeriodChart({
  series,
  metric,
}: {
  series: ReturnType<typeof buildPeriodSeries>;
  metric: ActivityMetric;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const { buckets } = series;
  const max = Math.max(...buckets.map((b) => b.value), 1);
  const dense = buckets.length > 7;
  const lastIndex = buckets.length - 1;
  const midIndex = Math.floor(lastIndex / 2);

  // The chart area stretches with the card's remaining height (single-screen fixed page): bar heights scale
  // as a percentage of the container instead of a fixed 100px.
  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 60 }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'stretch',
          gap: dense ? 2 : 8,
          flex: 1,
          minHeight: 0,
        }}
      >
        {buckets.map((bucket, i) => {
          const isToday = i === lastIndex;
          return (
            <div
              key={bucket.date}
              title={`${formatLocaleDate(bucket.date, locale, { dateStyle: 'medium' })} · ${formatMetricValue(bucket.value, metric, t, locale)}`}
              style={{
                flex: 1,
                minWidth: 0,
                height: '100%',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'flex-end',
                gap: 4,
              }}
            >
              {!dense && (
                <div
                  style={{
                    fontSize: 9.5,
                    color: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)',
                    fontWeight: isToday ? 'var(--ol-font-weight-semibold)' : 400,
                    flexShrink: 0,
                  }}
                >
                  {formatMetricValue(bucket.value, metric, t, locale)}
                </div>
              )}
              <div
                style={{
                  width: '100%',
                  height: `${(bucket.value / max) * 88}%`,
                  minHeight: 2,
                  borderRadius: dense ? 2 : 4,
                  background: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)',
                  opacity: bucket.value === 0 ? 0.15 : isToday ? 1 : 0.85,
                  transition:
                    'height 0.18s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-soft)',
                }}
              />
            </div>
          );
        })}
      </div>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          fontSize: 10,
          color: 'var(--ol-ink-4)',
          marginTop: 8,
          flexShrink: 0,
        }}
      >
        {dense
          ? [0, midIndex, lastIndex].map((i) => (
              <span key={i}>{formatLocaleDate(buckets[i].date, locale)}</span>
            ))
          : buckets.map((bucket) => (
              <span key={bucket.date}>
                {weekDayLabel(
                  bucket.date,
                  t('overview.weekDays', { returnObjects: true }) as string[],
                )}
              </span>
            ))}
      </div>
    </div>
  );
}

function weekDayLabel(dateKey: string, names: string[]): string {
  const [year, month, day] = dateKey.split('-').map(Number);
  return names[new Date(year, month - 1, day).getDay()];
}

/** Counts/chars display with thousands separators; duration converts to human-readable h/m/s. */
function formatMetricValue(
  value: number,
  metric: ActivityMetric,
  t: ReturnType<typeof useTranslation>['t'],
  locale: string,
): string {
  if (metric === 'duration') return formatLongDuration(value, t, locale);
  return formatLocaleNumber(Math.round(value), locale);
}

/** A period total can be tens of hours; formatDuration (seconds/minutes only) doesn't apply. */
function formatLongDuration(
  ms: number,
  t: ReturnType<typeof useTranslation>['t'],
  locale: string,
): string {
  if (ms <= 0) return '0';
  const totalSeconds = Math.round(ms / 1000);
  if (totalSeconds < 60)
    return t('common.durationSeconds', { value: formatLocaleNumber(totalSeconds, locale) });
  const totalMinutes = Math.floor(totalSeconds / 60);
  if (totalMinutes < 60)
    return t('overview.period.minutes', { value: formatLocaleNumber(totalMinutes, locale) });
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return t('overview.period.hoursMinutes', {
    hours: formatLocaleNumber(hours, locale),
    minutes: formatLocaleNumber(minutes, locale),
  });
}
