import { useTranslation } from 'react-i18next';
import {
  ACTIVITY_METRICS,
  ACTIVITY_PERIODS,
  buildPeriodSeries,
  type ActivityMetric,
  type ActivityPeriod,
} from '../../lib/activityMetrics';
import { formatLocaleDate, formatLocaleNumber } from '../../lib/localeFormat';
import { Button } from '../../ui/controls/Button';
import { Segmented } from '../../ui/controls/Segmented';
import { Card } from '../../ui/layout/Card';

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
    value: String(days),
    label: t(`overview.period.last${days}Days`),
  }));
  const metricOptions = ACTIVITY_METRICS.map((id) => ({
    value: id,
    label: t(`overview.metricName.${id}`),
  }));

  return (
    <Card
      className="ol-overview-period ol-col-5"
      padding={18}
      style={{
        display: 'flex',
        flexDirection: 'column',
        minWidth: 0,
        overflow: 'hidden',
      }}
    >
      <div className="ol-overview-period-controls">
        <Segmented
          value={String(period)}
          options={periodOptions}
          onChange={(value) => {
            const next = ACTIVITY_PERIODS.find((days) => String(days) === value);
            if (next !== undefined) onPeriodChange(next);
          }}
          label={t('overview.period.ariaLabel')}
        />
        <Segmented
          value={metric}
          options={metricOptions}
          onChange={onMetricChange}
          label={t('overview.metricName.ariaLabel')}
        />
      </div>

      {loadError ? (
        <div
          style={{
            minHeight: 0,
            flex: 1,
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
          <Button size="sm" onClick={onRetry}>
            {t('overview.historyRetry')}
          </Button>
        </div>
      ) : (
        <>
          <div className="ol-overview-period-summary">
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
    <div className="ol-overview-period-chart">
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
        className="ol-overview-period-dates"
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
