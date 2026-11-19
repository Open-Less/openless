import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { countCodePoints } from '../../lib/unicode';
import { formatLocaleDecimal, formatLocaleNumber } from '../../lib/localeFormat';
import type { DictationSession } from '../../api/contracts';
import { Card } from '../../ui/layout/Card';

export function TodayMetrics({
  mobile,
  history,
  historyError,
}: {
  mobile: boolean;
  history: DictationSession[];
  historyError: boolean;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const metrics = useMemo(() => {
    const date = new Date();
    date.setHours(0, 0, 0, 0);
    const todays = history.filter((entry) => new Date(entry.createdAt) >= date);
    const charsToday = todays.reduce((sum, entry) => sum + countCodePoints(entry.finalText), 0);
    const segmentsToday = todays.length;
    const totalDurationMs = todays.reduce((sum, entry) => sum + (entry.durationMs ?? 0), 0);
    return {
      charsToday,
      segmentsToday,
      totalDurationMs,
      avgLatencyMs: segmentsToday === 0 ? 0 : totalDurationMs / segmentsToday,
    };
  }, [history]);
  return (
    <section
      className="ol-grid-12 ol-grid-stats ol-overview-metrics"
      data-columns={mobile ? '2' : undefined}
      aria-label={t('overview.statsTitle')}
    >
      <Metric
        icon="hash"
        label={t('overview.metricChars')}
        value={historyError ? '—' : formatLocaleNumber(metrics.charsToday, locale)}
        trend={
          historyError
            ? t('overview.historyLoadError')
            : t('overview.metricSegments', { count: metrics.segmentsToday })
        }
      />
      <Metric
        icon="mic"
        label={t('overview.metricDuration')}
        value={historyError ? '—' : formatDuration(metrics.totalDurationMs, t, locale)}
        trend={historyError ? t('overview.historyLoadError') : ''}
      />
      <Metric
        icon="clock"
        label={t('overview.metricAvg')}
        value={historyError ? '—' : formatDuration(metrics.avgLatencyMs, t, locale)}
        trend={
          historyError
            ? t('overview.historyLoadError')
            : metrics.segmentsToday > 0
              ? t('overview.metricAvgTrend')
              : t('overview.metricNoData')
        }
      />
      <Metric
        icon="bolt"
        label={t('overview.metricTotal')}
        value={historyError ? '—' : formatLocaleNumber(history.length, locale)}
        trend={historyError ? t('overview.historyLoadError') : t('overview.metricTotalTrend')}
      />
    </section>
  );
}

interface MetricProps {
  icon: string;
  label: string;
  value: string;
  trend: string;
}

function Metric({ icon, label, value, trend }: MetricProps) {
  return (
    <Card className="ol-overview-metric" padding={14} style={{ minWidth: 0 }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          marginBottom: 8,
          color: 'var(--ol-ink-3)',
        }}
      >
        <Icon name={icon} size={13} />
        <span style={{ fontSize: 13 }}>{label}</span>
      </div>
      <div
        data-numeric
        style={{
          fontSize: 22,
          fontWeight: 'var(--ol-font-weight-semibold)',
          letterSpacing: '-0.02em',
          color: 'var(--ol-ink)',
          lineHeight: 1.2,
          overflowWrap: 'anywhere',
        }}
      >
        {value}
      </div>
      <div style={{ fontSize: 12, color: 'var(--ol-ink-4)', marginTop: 6 }}>{trend || ' '}</div>
    </Card>
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
