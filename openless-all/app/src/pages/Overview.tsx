// Overview.tsx — real metrics, derived from listHistory + getCredentials.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { getActivityStats, getCredentials, listHistory } from '../lib/ipc';
import { Heatmap } from '../components/Heatmap';
import { useMobileLayout } from '../lib/useMobileLayout';
import { countCodePoints } from '../lib/unicode';
import {
  formatHistoryTime,
  formatLocaleDate,
  formatLocaleDecimal,
  formatLocaleNumber,
} from '../lib/localeFormat';
import { isDesktop } from '../lib/platform';
import { getOverviewSetup, type OverviewSettingsSection } from '../lib/overviewSetup';
import {
  ACTIVITY_METRICS,
  ACTIVITY_PERIODS,
  buildPeriodSeries,
  type ActivityMetric,
  type ActivityPeriod,
} from '../lib/activityMetrics';
import type { ActivityDay, CredentialsStatus, DictationSession, PolishMode } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';
import { ASR_LABELS } from './settings/shared';

function useModeLabels(): Record<PolishMode, string> {
  const { t } = useTranslation();
  return {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
}

interface OverviewProps {
  onOpenHistory?: () => void;
  onOpenSettings?: (section: 'general' | 'services' | 'privacy' | 'shortcuts') => void;
}

// id → i18n nameKey; this keeps display labels only, provider behavior comes from the Core descriptor.
// (bailian-qwen3-realtime / apple-speech were once missing and fell back to showing the bare id).
const ASR_NAME_KEY_BY_ID: Record<string, string> = Object.fromEntries(
  ASR_LABELS.map((p) => [p.id, p.nameKey]),
);

const LLM_NAME_KEY_BY_ID: Record<string, string> = {
  ark: 'ark',
  deepseek: 'deepseek',
  siliconflow: 'siliconflow',
  atlascloud: 'atlascloud',
  openai: 'openai',
  gemini: 'gemini',
  codex_oauth: 'codexOAuth',
  mimo: 'mimo',
  cometapi: 'cometapi',
  openrouterFree: 'openrouterFree',
  requesty: 'requesty',
  'api-route': 'apiRoute',
  orcarouter: 'orcarouter',
  alibabaCoding: 'alibabaCoding',
  codingPlanX: 'codingPlanX',
  minimax: 'minimax',
  stepfun: 'stepfun',
  opencode: 'opencode',
  tencentTokenHub: 'tencentTokenHub',
  custom: 'custom',
};

export function Overview({ onOpenHistory, onOpenSettings }: OverviewProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const mobile = useMobileLayout();
  const modeLabel = useModeLabels();
  const [history, setHistory] = useState<DictationSession[]>([]);
  const [historyError, setHistoryError] = useState(false);
  const [credsError, setCredsError] = useState(false);
  const [credsLoading, setCredsLoading] = useState(true);
  const [creds, setCreds] = useState<CredentialsStatus | null>(null);
  const { prefs, capability } = useHotkeySettings();
  // A narrow desktop window still uses desktop shortcuts.
  const desktop = isDesktop();
  const credentialsRequestSeq = useRef(0);
  const historyRequestSeq = useRef(0);
  const activityRequestSeq = useRef(0);

  const refreshHistory = useCallback(() => {
    const requestSeq = historyRequestSeq.current + 1;
    historyRequestSeq.current = requestSeq;
    setHistoryError(false);
    listHistory()
      .then((entries) => {
        if (requestSeq !== historyRequestSeq.current) return;
        setHistory(entries);
      })
      .catch((error) => {
        if (requestSeq !== historyRequestSeq.current) return;
        console.error('[overview] failed to load history', error);
        setHistoryError(true);
      });
  }, []);

  // Activity data (stored independently of history content; clearing history doesn't affect it): shared by the
  // yearly heatmap and the 7/30-day metrics. Load failure only hides the corresponding cards.
  //
  // The heatmap doesn't render on mobile (issue #861: fixed horizontal width overflows narrow screens and slows
  // the WebView), but the period metric cards do render, so the IPC can no longer skip on mobile — otherwise
  // mobile period cards would stay empty forever.
  const [activity, setActivity] = useState<ActivityDay[] | null>(null);
  const [activityError, setActivityError] = useState(false);
  const refreshActivity = useCallback(() => {
    const requestSeq = activityRequestSeq.current + 1;
    activityRequestSeq.current = requestSeq;
    setActivityError(false);
    getActivityStats()
      .then((stats) => {
        if (requestSeq !== activityRequestSeq.current) return;
        setActivity(stats);
      })
      .catch((error) => {
        if (requestSeq !== activityRequestSeq.current) return;
        console.error('[overview] failed to load activity stats', error);
        setActivity(null);
        setActivityError(true);
      });
  }, []);
  useEffect(() => {
    refreshActivity();
  }, [refreshActivity]);

  const refreshCredentials = useCallback(() => {
    const requestSeq = credentialsRequestSeq.current + 1;
    credentialsRequestSeq.current = requestSeq;
    setCredsError(false);
    setCredsLoading(true);
    getCredentials()
      .then((status) => {
        if (requestSeq !== credentialsRequestSeq.current) return;
        setCreds(status);
        setCredsError(false);
      })
      .catch((error) => {
        if (requestSeq !== credentialsRequestSeq.current) return;
        console.error('[overview] failed to load credentials status', error);
        setCredsError(true);
      })
      .finally(() => {
        if (requestSeq === credentialsRequestSeq.current) setCredsLoading(false);
      });
  }, []);

  useEffect(() => {
    refreshHistory();
  }, [refreshHistory]);

  useEffect(() => {
    refreshCredentials();
  }, [
    refreshCredentials,
    prefs?.activeLlmProvider,
    prefs?.activeAsrProvider,
    prefs?.pipelineMode,
    prefs?.activeOmniProvider,
  ]);

  // ⌘R / Ctrl+R refetches this page's three data sets (history, activity, credentials), same key and semantics as the history page.
  // preventDefault blocks the webview's default full-page reload, which would remount the whole frontend.
  // The overview previously had no refresh entry; users had to switch pages away and back to see fresh data.
  const refreshAll = useCallback(() => {
    refreshHistory();
    refreshActivity();
    refreshCredentials();
  }, [refreshHistory, refreshActivity, refreshCredentials]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'r' || e.key === 'R')) {
        e.preventDefault();
        refreshAll();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [refreshAll]);

  // Refetch status after credentials are saved (issue #532 / #573: filling/updating credentials in Settings
  // without switching providers doesn't re-run the useEffect above, leaving the overview stuck on "not configured").
  // Reuses refreshCredentials() to keep the credentialsRequestSeq race guard.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen('credentials:changed', () => {
          if (cancelled) return;
          refreshCredentials();
        });
        if (cancelled) {
          handle();
        } else {
          unlisten = handle;
        }
      } catch {
        // browser dev mock — no Tauri event bridge
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refreshCredentials]);

  const metrics = useMemo(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const todays = history.filter((s) => new Date(s.createdAt) >= today);
    const charsToday = todays.reduce((acc, s) => acc + countCodePoints(s.finalText), 0);
    const segmentsToday = todays.length;
    const totalDurationMs = todays.reduce((acc, s) => acc + (s.durationMs ?? 0), 0);
    const avgLatencyMs = segmentsToday > 0 ? totalDurationMs / segmentsToday : 0;
    return { charsToday, segmentsToday, totalDurationMs, avgLatencyMs };
  }, [history]);

  // Period metrics: last 7 / 30 days × count / chars / duration.
  //
  // The data source must be activity, not history — history has a 200-entry hard cap, so a user with hundreds of
  // daily sessions pushes last week out within days; computing from history would draw data-less days as 0 while
  // the yearly heatmap on the same page shows them lit, contradicting itself.
  const [period, setPeriod] = useState<ActivityPeriod>(7);
  const [metric, setMetric] = useState<ActivityMetric>('count');
  const series = useMemo(
    () => buildPeriodSeries(activity ?? [], period, metric),
    [activity, period, metric],
  );

  const setup = getOverviewSetup({
    credentials: creds,
    loading: credsLoading,
    error: credsError,
    omniProvider: prefs?.activeOmniProvider,
    desktop,
    hotkeyAvailable: prefs && capability ? capability.adapter !== 'unavailable' : null,
    hasShortcut: Boolean(prefs?.dictationHotkey.primary.trim()),
  });
  const openSettings = (section: OverviewSettingsSection) => onOpenSettings?.(section);
  // Configured provider cards no longer stay resident (no informational value); only still-pending cards
  // show as reminders. Once everything is configured the whole "current voice services" group hides. While
  // credentials are loading / the fetch failed (providers empty), keep placeholder cards to avoid a blank flash.
  const pendingProviders = setup.providers.filter((p) => !p.configured);
  const showProvidersSection = setup.providers.length === 0 || pendingProviders.length > 0;

  return (
    // Single-screen fixed page: no scrolling, fills the height given by the shell, dashboards share
    // one screen elastically; when the window is squeezed very short, the bottom row shrinks internally
    // (the recent list scrolls internally) and the page itself shows no scrollbar.
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        flex: 1,
        minHeight: 0,
        minWidth: 0,
        gap: 14,
      }}
    >
      <PageHeader
        compact
        title={t('overview.title')}
        right={
          <Btn size="sm" icon="refresh" onClick={refreshAll}>
            {t('overview.refresh')}
          </Btn>
        }
      />

      {showProvidersSection && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8, flexShrink: 0 }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'baseline',
              justifyContent: 'space-between',
              flexWrap: 'wrap',
              gap: 8,
            }}
          >
            <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink-2)', margin: 0 }}>
              {t('overview.servicesTitle')}
            </h2>
            <Btn
              size="sm"
              variant="soft"
              disabled={!onOpenSettings}
              onClick={() => openSettings('services')}
            >
              {t('overview.actions.services')}
            </Btn>
          </div>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns:
                mobile || pendingProviders.length < 2
                  ? 'minmax(0, 1fr)'
                  : 'repeat(2, minmax(0, 1fr))',
              gap: 12,
            }}
          >
            {pendingProviders.map((provider) => {
              const nameKey =
                provider.id &&
                (provider.kind === 'asr' ? ASR_NAME_KEY_BY_ID : LLM_NAME_KEY_BY_ID)[provider.id];
              const name = nameKey
                ? t(`settings.providers.presets.${nameKey}`)
                : provider.id ||
                  t(provider.kind === 'omni' ? 'overview.omniName' : 'overview.statusUnknown');
              return (
                <ProviderCard
                  key={provider.kind}
                  kind={provider.kind}
                  name={name}
                  status="notConfigured"
                  onConfigure={onOpenSettings ? () => openSettings('services') : undefined}
                />
              );
            })}
            {setup.providers.length === 0 && (
              <Card padding={16}>
                <div role="status" style={{ fontSize: 13, color: 'var(--ol-ink-3)' }}>
                  {t(credsLoading ? 'overview.statusLoading' : 'overview.credentialsLoadError')}
                </div>
              </Card>
            )}
          </div>
        </div>
      )}

      {/* Usage records: title + four metric cards as one group. */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, flexShrink: 0 }}>
        <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink-2)', margin: 0 }}>
          {t('overview.statsTitle')}
        </h2>
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: mobile ? 'repeat(2, minmax(0, 1fr))' : 'repeat(4, minmax(0, 1fr))',
            gap: 12,
          }}
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
        </div>
      </div>

      {/* Bottom row takes the remaining height: the period chart area stretches, the recent list scrolls internally. */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: mobile ? 'minmax(0, 1fr)' : 'minmax(0, 1fr) minmax(0, 1.4fr)',
          gap: 12,
          flex: 1,
          minHeight: 0,
        }}
      >
        <PeriodMetricsCard
          series={series}
          period={period}
          metric={metric}
          onPeriodChange={setPeriod}
          onMetricChange={setMetric}
          loadError={activityError}
          onRetry={refreshActivity}
        />

        <Card
          padding={0}
          style={{ display: 'flex', flexDirection: 'column', minWidth: 0, overflow: 'hidden' }}
        >
          <div
            style={{
              padding: '12px 18px',
              borderBottom: '0.5px solid var(--ol-line)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              flexShrink: 0,
            }}
          >
            <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink-2)' }}>
              {t('overview.recentTitle')}
            </span>
            <Btn size="sm" variant="ghost" disabled={!onOpenHistory} onClick={onOpenHistory}>
              {t('overview.recentAll')}
            </Btn>
          </div>
          <div className="ol-thinscroll" style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
            {historyError ? (
              <div
                style={{
                  padding: 24,
                  textAlign: 'center',
                  fontSize: 12,
                  color: 'var(--ol-ink-4)',
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: 'center',
                  gap: 10,
                }}
              >
                <span>{t('overview.recentLoadFailed')}</span>
                <Btn size="sm" variant="ghost" onClick={refreshHistory}>
                  {t('overview.historyRetry')}
                </Btn>
              </div>
            ) : (
              <>
                {history.length === 0 && (
                  <div
                    style={{
                      padding: 24,
                      textAlign: 'center',
                      fontSize: 12,
                      color: 'var(--ol-ink-4)',
                    }}
                  >
                    {t('overview.recentEmptyHint')}
                  </div>
                )}
                {history.slice(0, 8).map((s) => (
                  <RecentRow key={s.id} session={s} modeLabel={modeLabel} />
                ))}
              </>
            )}
          </div>
        </Card>
      </div>
      {/* Independent activity storage survives history clearing; keep the existing visibility preference. */}
      {!mobile &&
        prefs?.showOverviewActivityHeatmap !== false &&
        activity &&
        activity.length > 0 && <ActivityHeatmapCard activity={activity} />}
    </div>
  );
}

interface ProviderCardProps {
  kind: 'asr' | 'llm' | 'omni';
  name: string;
  status: 'configured' | 'notConfigured';
  onConfigure?: () => void;
}

function ProviderCard({ kind, name, status, onConfigure }: ProviderCardProps) {
  const { t } = useTranslation();
  return (
    <Card padding={16} style={{ display: 'flex', flexDirection: 'column', gap: 12, minWidth: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <div
          style={{
            width: 38,
            height: 38,
            borderRadius: 10,
            flexShrink: 0,
            background: 'var(--ol-blue-soft)',
            color: 'var(--ol-blue)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <Icon name={kind === 'asr' ? 'mic' : 'sparkle'} size={18} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              flexWrap: 'wrap',
              gap: 8,
              marginBottom: 4,
            }}
          >
            <span style={{ fontSize: 12.5, color: 'var(--ol-ink-4)', fontWeight: 600 }}>
              {t(`overview.${kind}Kind`)}
            </span>
            {status === 'configured' && (
              <Pill tone="ok" size="sm">
                <span
                  style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-ok)' }}
                />
                {t('overview.statusConfigured')}
              </Pill>
            )}
            {status === 'notConfigured' && (
              <Pill tone="outline" size="sm">
                {t('overview.statusNotConfigured')}
              </Pill>
            )}
          </div>
          <div
            style={{
              fontSize: 15,
              fontWeight: 600,
              color: 'var(--ol-ink)',
              overflowWrap: 'anywhere',
            }}
          >
            {name}
          </div>
        </div>
      </div>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          flexWrap: 'wrap',
          gap: 8,
        }}
      >
        <span
          style={{ flex: '1 1 160px', fontSize: 13, color: 'var(--ol-ink-3)', lineHeight: 1.5 }}
        >
          {t(`overview.providerHelp.${kind}`)}
        </span>
        <Btn size="sm" icon="chevRight" disabled={!onConfigure} onClick={onConfigure}>
          {t(status === 'configured' ? 'overview.manageProvider' : 'overview.configureProvider')}
        </Btn>
      </div>
    </Card>
  );
}

/** Yearly activity heatmap card: dictation count per day over the past 365 days. Month/weekday/date labels are generated via Intl in the current language. */
function ActivityHeatmapCard({ activity }: { activity: ActivityDay[] }) {
  const { t, i18n } = useTranslation();
  const { endDate, startDate, data, labels } = useMemo(() => {
    // The year calendar spans the calendar year — from Jan 1 to Dec 31, filled left to right;
    // the previous rolling 365-day window started month labels mid-year and left the right side empty, looking like missing data.
    const now = new Date();
    const year = now.getFullYear();
    const start = new Date(year, 0, 1);
    const end = new Date(year, 11, 31);
    const lang = i18n.resolvedLanguage || i18n.language || 'en';
    const monthFormat = new Intl.DateTimeFormat(lang, { month: 'short' });
    const dayFormat = new Intl.DateTimeFormat(lang, { weekday: 'short' });
    const dateFormat = new Intl.DateTimeFormat(lang, { dateStyle: 'medium' });
    const anchor = new Date(year, 0, 4); // Sunday
    return {
      endDate: end,
      startDate: start,
      data: activity.map((day) => ({ date: day.date, value: day.count })),
      labels: {
        months: Array.from({ length: 12 }, (_, m) => monthFormat.format(new Date(year, m, 1))),
        days: Array.from({ length: 7 }, (_, d) => {
          const date = new Date(anchor);
          date.setDate(anchor.getDate() + d);
          return dayFormat.format(date);
        }),
        date: (date: Date) => dateFormat.format(date),
      },
    };
  }, [activity, i18n.resolvedLanguage, i18n.language]);
  return (
    <Card padding={12} style={{ display: 'flex', flexDirection: 'column', gap: 6, minWidth: 0 }}>
      <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink-2)', flexShrink: 0 }}>
        {t('overview.activityTitle')}
      </span>
      {/* Cells no longer cap: 53 columns fill the card width to the right edge.
          The card height tightens to content height (flex default 0 1 auto: no stretch, shrinks only when
          the window is squeezed short); padding is equal on all four sides; the freed height goes entirely
          to the period/recent rows above, and the heatmap sinks to the bottom. */}
      <Heatmap
        data={data}
        startDate={startDate}
        endDate={endDate}
        monthLabels={labels.months}
        dayLabels={labels.days}
        dateDisplay={labels.date}
        valueDisplay={(count) => t('overview.activityCount', { count })}
      />
    </Card>
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
    <Card padding={14} style={{ minWidth: 0 }}>
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
        style={{
          fontSize: 22,
          fontWeight: 600,
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
      role="group"
      aria-label={ariaLabel}
      style={{
        display: 'flex',
        gap: 2,
        padding: 2,
        borderRadius: 8,
        background: 'var(--ol-surface-2)',
        border: '0.5px solid var(--ol-line)',
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
              fontWeight: selected ? 600 : 500,
              border: 0,
              borderRadius: 6,
              background: selected ? 'var(--ol-blue)' : 'transparent',
              color: selected ? '#fff' : 'var(--ol-ink-3)',
              cursor: 'pointer',
              fontFamily: 'inherit',
              whiteSpace: 'nowrap',
              transition:
                'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
            }}
          >
            {option.label}
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
function PeriodMetricsCard({
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
      padding={18}
      style={{ display: 'flex', flexDirection: 'column', minWidth: 0, minHeight: 0 }}
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
                fontWeight: 600,
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
                    fontWeight: isToday ? 600 : 400,
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

  const onCopy = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error('clipboard unavailable');
      // Same as History: when polish failed / produced nothing, finalText is empty and the copy falls back to
      // the raw transcript, avoiding copying an empty string.
      await navigator.clipboard.writeText(
        session.finalText.trim() ? session.finalText : session.rawTranscript,
      );
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('[overview] failed to copy recent entry', error);
    }
  };

  return (
    <div
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
          style={{ fontSize: 12.5, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}
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
        <Btn
          size="sm"
          variant="ghost"
          icon={copied ? 'check' : 'copy'}
          onClick={() => void onCopy()}
          style={{ padding: '3px 8px' }}
        >
          {copied ? t('common.copied') : t('common.copy')}
        </Btn>
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
