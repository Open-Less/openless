// Overview.tsx — 真实指标，从 listHistory + getCredentials 派生。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { formatComboLabel } from '../lib/hotkey';
import { getActivityStats, getCredentials, listHistory } from '../lib/ipc';
import { Heatmap } from '../components/Heatmap';
import { useMobileLayout } from '../lib/useMobileLayout';
import { countCodePoints } from '../lib/unicode';
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
import { ASR_PRESETS } from './settings/shared';

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
}

// id → i18n nameKey，从 ASR_PRESETS 单一来源派生，避免这里再手维护一份 id 列表
// （之前漏了 bailian-qwen3-realtime / apple-speech，会退化成显示裸 id）。
const ASR_NAME_KEY_BY_ID: Record<string, string> = Object.fromEntries(
  ASR_PRESETS.map(p => [p.id, p.nameKey]),
);

const LLM_NAME_KEY_BY_ID: Record<string, string> = {
  ark: 'ark',
  deepseek: 'deepseek',
  siliconflow: 'siliconflow',
  atlascloud: 'atlascloud',
  openai: 'openai',
  codex_oauth: 'codexOAuth',
  mimo: 'mimo',
  cometapi: 'cometapi',
  openrouterFree: 'openrouterFree',
  alibabaCoding: 'alibabaCoding',
  codingPlanX: 'codingPlanX',
  custom: 'custom',
};

export function Overview({ onOpenHistory }: OverviewProps) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const modeLabel = useModeLabels();
  const [history, setHistory] = useState<DictationSession[]>([]);
  const [historyError, setHistoryError] = useState(false);
  const [credsError, setCredsError] = useState(false);
  const [creds, setCreds] = useState<CredentialsStatus>({
    activeAsrProvider: 'volcengine',
    activeLlmProvider: 'ark',
    pipelineMode: 'traditional',
    asrConfigured: false,
    llmConfigured: false,
    omniConfigured: false,
    volcengineConfigured: false,
    arkConfigured: false,
  });
  const { prefs } = useHotkeySettings();
  const credentialsRequestSeq = useRef(0);
  const historyRequestSeq = useRef(0);
  const activityRequestSeq = useRef(0);

  const refreshHistory = useCallback(() => {
    const requestSeq = historyRequestSeq.current + 1;
    historyRequestSeq.current = requestSeq;
    setHistoryError(false);
    listHistory()
      .then(entries => {
        if (requestSeq !== historyRequestSeq.current) return;
        setHistory(entries);
      })
      .catch(error => {
        if (requestSeq !== historyRequestSeq.current) return;
        console.error('[overview] failed to load history', error);
        setHistoryError(true);
      });
  }, []);

  // 活动数据（独立于历史内容存储，清空历史不影响）：年度热力图 + 近 7/30 天指标共用。
  // 加载失败仅隐藏对应卡片。
  //
  // 热力图在移动端不渲染（issue #861：横向宽度固定，窄屏易溢出并拖慢 WebView），但
  // 周期指标卡是要渲染的，所以 IPC 不能再按 mobile 跳过 —— 否则移动端周期卡永远空。
  const [activity, setActivity] = useState<ActivityDay[] | null>(null);
  const [activityError, setActivityError] = useState(false);
  const refreshActivity = useCallback(() => {
    const requestSeq = activityRequestSeq.current + 1;
    activityRequestSeq.current = requestSeq;
    setActivityError(false);
    getActivityStats()
      .then(stats => {
        if (requestSeq !== activityRequestSeq.current) return;
        setActivity(stats);
      })
      .catch(error => {
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
    getCredentials()
      .then(status => {
        if (requestSeq !== credentialsRequestSeq.current) return;
        setCreds(status);
        setCredsError(false);
      })
      .catch(error => {
        if (requestSeq !== credentialsRequestSeq.current) return;
        console.error('[overview] failed to load credentials status', error);
        setCredsError(true);
      });
  }, []);

  useEffect(() => {
    refreshHistory();
  }, [refreshHistory]);

  useEffect(() => {
    refreshCredentials();
  }, [refreshCredentials, prefs?.activeLlmProvider, prefs?.activeAsrProvider]);

  // ⌘R / Ctrl+R 重新拉取本页的三份数据（历史、活动、凭据），与历史页同键同语义。
  // preventDefault 拦掉 webview 默认的整页 reload，避免整个前端重挂载。
  // 此前概览页没有刷新入口，用户只能切到别的页再切回来才能看到新数据。
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

  // 凭据被保存后重新拉取状态（issue #532 / #573：在 Settings 中填写/更新凭据
  // 但不切换提供商时，上面的 useEffect 不会重跑，导致概览页的状态仍停留在「未配置」）。
  // 复用 refreshCredentials() 以带上 credentialsRequestSeq 防竞态。
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
        // browser dev mock — 没有 Tauri event bridge
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
    const todays = history.filter(s => new Date(s.createdAt) >= today);
    const charsToday = todays.reduce((acc, s) => acc + countCodePoints(s.finalText), 0);
    const segmentsToday = todays.length;
    const totalDurationMs = todays.reduce((acc, s) => acc + (s.durationMs ?? 0), 0);
    const avgLatencyMs = segmentsToday > 0 ? totalDurationMs / segmentsToday : 0;
    return { charsToday, segmentsToday, totalDurationMs, avgLatencyMs };
  }, [history]);

  // 周期指标：近 7 天 / 近 30 天 × 条数 / 字数 / 时长。
  //
  // 数据源必须是 activity 而不是 history —— history 有 200 条硬上限，日均上百次的用户
  // 两三天就把上周挤没了，按历史现算会把没数据的那几天画成 0（而同一页的年度热力图
  // 上那几天明明是亮的，两块数据自相矛盾）。
  const [period, setPeriod] = useState<ActivityPeriod>(7);
  const [metric, setMetric] = useState<ActivityMetric>('count');
  const series = useMemo(
    () => buildPeriodSeries(activity ?? [], period, metric),
    [activity, period, metric],
  );

  const asrProviderId = creds.activeAsrProvider || 'volcengine';
  const llmProviderId = creds.activeLlmProvider || 'ark';
  const asrNameKey = ASR_NAME_KEY_BY_ID[asrProviderId];
  const llmNameKey = LLM_NAME_KEY_BY_ID[llmProviderId];
  const asrProviderName = asrNameKey
    ? t(`settings.providers.presets.${asrNameKey}`)
    : asrProviderId;
  const llmProviderName = llmNameKey
    ? t(`settings.providers.presets.${llmNameKey}`)
    : llmProviderId;

  return (
    <>
      <PageHeader title={t('overview.title')} />

      <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1fr', gap: 12, marginBottom: 18 }}>
        <ProviderCard
          kind={t('overview.asrKind')}
          name={asrProviderName}
          subname={asrProviderId}
          status={credsError ? 'error' : creds.asrConfigured ? 'configured' : 'notConfigured'}
        />
        <ProviderCard
          kind={t('overview.llmKind')}
          name={llmProviderName}
          subname={llmProviderId}
          status={credsError ? 'error' : creds.llmConfigured ? 'configured' : 'notConfigured'}
        />
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: mobile ? 'repeat(2, 1fr)' : 'repeat(4, 1fr)', gap: 12, marginBottom: 18 }}>
        <Metric icon="hash" label={t('overview.metricChars')} value={historyError ? '—' : metrics.charsToday.toLocaleString()} trend={historyError ? t('overview.historyLoadError') : t('overview.metricSegments', { count: metrics.segmentsToday })} />
        <Metric icon="mic" label={t('overview.metricDuration')} value={historyError ? '—' : formatDuration(metrics.totalDurationMs, t)} trend={historyError ? t('overview.historyLoadError') : ''} />
        <Metric icon="clock" label={t('overview.metricAvg')} value={historyError ? '—' : formatDuration(metrics.avgLatencyMs, t)} trend={historyError ? t('overview.historyLoadError') : metrics.segmentsToday > 0 ? t('overview.metricAvgTrend') : t('overview.metricNoData')} />
        <Metric icon="bolt" label={t('overview.metricTotal')} value={historyError ? '—' : String(history.length)} trend={historyError ? t('overview.historyLoadError') : t('overview.metricTotalTrend')} accent />
      </div>

      {/* 年度活动热力图（8starlabs Heatmap 规格）：过去一年每日听写次数。
          数据来自独立的 activity 计数存储，与历史保留策略解耦；
          可在设置 → 通用 → 外观 里单独关闭。
          移动端（useMobileLayout）不渲染，避免窄屏横向溢出（issue #861）。 */}
      {!mobile &&
        prefs?.showOverviewActivityHeatmap !== false &&
        activity &&
        activity.length > 0 && (
          <ActivityHeatmapCard activity={activity} />
        )}

      {/* 底部一行 = flex:1 撑满剩余高度（父 wrapper 是 display:flex/column）。
          只有「最近识别」内部允许滚动；其他卡片按内容自然高度，不破裂底部圆角。
          issue #243 follow-up：去掉外层 overflow 后底部圆角被裁的视觉问题。 */}
      <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1.4fr', gap: 12, flex: mobile ? undefined : 1, minHeight: mobile ? undefined : 0 }}>
        {/* overflow:hidden：窗口过小时这一行 flex:1 会被压到比内容还矮，柱状图（固定高）
            原本会溢出卡片圆角外（issue #782）。裁进卡片内，与右侧「最近识别」卡片一致。 */}
        <PeriodMetricsCard
          series={series}
          period={period}
          metric={metric}
          onPeriodChange={setPeriod}
          onMetricChange={setMetric}
          loadError={activityError}
        />

        <Card padding={0} style={{ display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' }}>
          <div style={{ padding: '14px 18px', borderBottom: '0.5px solid var(--ol-line)', display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexShrink: 0 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>{t('overview.recentTitle')}</span>
            <Btn size="sm" variant="ghost" onClick={onOpenHistory}>{t('overview.recentAll')}</Btn>
          </div>
          <div className="ol-thinscroll" style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
            {historyError ? (
              <div style={{ padding: 24, textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10 }}>
                <span>{t('overview.recentLoadFailed')}</span>
                <Btn size="sm" variant="ghost" onClick={refreshHistory}>{t('overview.historyRetry')}</Btn>
              </div>
            ) : (
              <>
                {history.length === 0 && (
                  <div style={{ padding: 24, textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>
                    {t('overview.recentEmpty', { trigger: prefs ? formatComboLabel(prefs.dictationHotkey) : '' })}
                  </div>
                )}
                {history.slice(0, 5).map(s => (
                  <RecentRow key={s.id} session={s} modeLabel={modeLabel} />
                ))}
              </>
            )}
          </div>
        </Card>
      </div>
    </>
  );
}

interface ProviderCardProps {
  kind: string;
  name: string;
  subname: string;
  status: 'configured' | 'notConfigured' | 'error';
}

function ProviderCard({ kind, name, subname, status }: ProviderCardProps) {
  const { t } = useTranslation();
  // ASR 卡用 mic 图标，其他用 sparkle —— 通过比较译文判断会随语言改变，故改用本地化无关的字面量比较。
  const isAsr = kind === t('overview.asrKind');
  return (
    <Card padding={16} style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
      <div
        style={{
          width: 38, height: 38, borderRadius: 10,
          background: 'var(--ol-blue-soft)',
          color: 'var(--ol-blue)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}
      >
        <Icon name={isAsr ? 'mic' : 'sparkle'} size={18} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 2 }}>
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', fontWeight: 600, letterSpacing: '.06em', textTransform: 'uppercase' }}>{kind}</span>
          {status === 'configured' && (
            <Pill tone="ok" size="sm">
              <span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-ok)' }} />
              {t('overview.statusConfigured')}
            </Pill>
          )}
          {status === 'notConfigured' && (
            <Pill tone="outline" size="sm">{t('overview.statusNotConfigured')}</Pill>
          )}
          {status === 'error' && (
            <Pill tone="outline" size="sm" style={{ color: 'var(--ol-red, #ef4444)', borderColor: 'rgba(239,68,68,0.24)' }}>{t('overview.statusUnknown')}</Pill>
          )}
        </div>
        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{name}</div>
        <div style={{ fontSize: 11.5, color: status === 'error' ? 'var(--ol-red, #ef4444)' : 'var(--ol-ink-3)', marginTop: 1, fontFamily: status === 'error' ? undefined : 'var(--ol-font-mono)' }}>
          {status === 'error' ? t('overview.credentialsLoadError') : subname}
        </div>
      </div>
    </Card>
  );
}

/** 年度活动热力图卡：过去 365 天每日听写次数。月份/星期/日期标签用 Intl 按当前语言生成。 */
function ActivityHeatmapCard({ activity }: { activity: ActivityDay[] }) {
  const { t, i18n } = useTranslation();
  const { endDate, startDate, data, labels } = useMemo(() => {
    const end = new Date();
    const start = new Date(end);
    start.setDate(end.getDate() - 364);
    const lang = i18n.language || 'en';
    const monthFormat = new Intl.DateTimeFormat(lang, { month: 'short' });
    const dayFormat = new Intl.DateTimeFormat(lang, { weekday: 'short' });
    const dateFormat = new Intl.DateTimeFormat(lang, { dateStyle: 'medium' });
    const anchor = new Date(2026, 0, 4); // 周日
    return {
      endDate: end,
      startDate: start,
      data: activity.map(day => ({ date: day.date, value: day.count })),
      labels: {
        months: Array.from({ length: 12 }, (_, m) => monthFormat.format(new Date(2026, m, 1))),
        days: Array.from({ length: 7 }, (_, d) => {
          const date = new Date(anchor);
          date.setDate(anchor.getDate() + d);
          return dayFormat.format(date);
        }),
        date: (date: Date) => dateFormat.format(date),
      },
    };
  }, [activity, i18n.language]);
  return (
    // marginBottom 与上方 provider / metrics 两行的 18px 节奏保持一致：否则「年度活动」
    // 会直接贴住下面「近 7 天 / 最近识别」那一行，中间没有间隔（issue #781）。
    <Card padding={18} style={{ display: 'flex', flexDirection: 'column', gap: 12, marginBottom: 18 }}>
      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>
        {t('overview.activityTitle')}
      </span>
      <Heatmap
        data={data}
        startDate={startDate}
        endDate={endDate}
        monthLabels={labels.months}
        dayLabels={labels.days}
        dateDisplay={labels.date}
        valueDisplay={count => t('overview.activityCount', { count })}
      />
    </Card>
  );
}

interface MetricProps {
  icon: string;
  label: string;
  value: string;
  trend: string;
  accent?: boolean;
}

function Metric({ icon, label, value, trend, accent }: MetricProps) {
  return (
    <Card padding={16}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, color: 'var(--ol-ink-3)' }}>
        <Icon name={icon} size={13} />
        <span style={{ fontSize: 11.5 }}>{label}</span>
      </div>
      <div style={{ fontSize: 26, fontWeight: 600, letterSpacing: '-0.02em', color: accent ? 'var(--ol-blue)' : 'var(--ol-ink)', lineHeight: 1.1 }}>{value}</div>
      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 6 }}>{trend || ' '}</div>
    </Card>
  );
}

/** 分段切换器（周期 / 指标共用）。窄，一行放得下两组。 */
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
      {options.map(option => {
        const selected = option.value === value;
        return (
          <button
            key={String(option.value)}
            type="button"
            aria-pressed={selected}
            onClick={() => onChange(option.value)}
            style={{
              padding: '3px 9px',
              fontSize: 11,
              fontWeight: selected ? 600 : 500,
              border: 0,
              borderRadius: 6,
              background: selected ? 'var(--ol-blue)' : 'transparent',
              color: selected ? '#fff' : 'var(--ol-ink-3)',
              cursor: 'pointer',
              fontFamily: 'inherit',
              whiteSpace: 'nowrap',
              transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
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
 * 周期指标卡：近 7 天 / 近 30 天 × 条数 / 字数 / 时长。
 *
 * 卡片顶部显示周期总计（大字）+ 日均，柱状图在下 —— 用户关心的「这个月总共说了多少字」
 * 是一个数，不是要在 30 根柱子里目测求和。
 */
function PeriodMetricsCard({
  series,
  period,
  metric,
  onPeriodChange,
  onMetricChange,
  loadError,
}: {
  series: ReturnType<typeof buildPeriodSeries>;
  period: ActivityPeriod;
  metric: ActivityMetric;
  onPeriodChange: (next: ActivityPeriod) => void;
  onMetricChange: (next: ActivityMetric) => void;
  loadError: boolean;
}) {
  const { t } = useTranslation();
  const periodOptions = ACTIVITY_PERIODS.map(days => ({
    value: days,
    label: t(`overview.period.last${days}Days`),
  }));
  const metricOptions = ACTIVITY_METRICS.map(id => ({
    value: id,
    label: t(`overview.metricName.${id}`),
  }));

  return (
    <Card padding={18} style={{ display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' }}>
      {/* flexWrap：卡片在 1fr 列里较窄，两组切换器放不下时换行而不是压扁按钮。 */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, flexWrap: 'wrap', marginBottom: 12 }}>
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
        <div style={{ height: 132, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>
          {t('overview.activityLoadError')}
        </div>
      ) : (
        <>
          <div style={{ marginBottom: 12 }}>
            <div style={{ fontSize: 26, fontWeight: 600, letterSpacing: '-0.02em', color: 'var(--ol-ink)', lineHeight: 1.1 }}>
              {formatMetricValue(series.total, metric, t)}
            </div>
            <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 5 }}>
              {t('overview.period.dailyAverage', {
                value: formatMetricValue(series.dailyAverage, metric, t),
              })}
            </div>
          </div>
          <PeriodChart series={series} metric={metric} />
        </>
      )}
    </Card>
  );
}

/** 柱状图。7 天时每根柱子上标数值；30 天时柱子只有几像素宽，标了会糊成一片，
 *  改用 title 悬浮显示，并只在两端和中间标日期。 */
function PeriodChart({
  series,
  metric,
}: {
  series: ReturnType<typeof buildPeriodSeries>;
  metric: ActivityMetric;
}) {
  const { t } = useTranslation();
  const { buckets } = series;
  const max = Math.max(...buckets.map(b => b.value), 1);
  const dense = buckets.length > 7;
  const lastIndex = buckets.length - 1;
  const midIndex = Math.floor(lastIndex / 2);

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: dense ? 2 : 8, height: 100 }}>
        {buckets.map((bucket, i) => {
          const isToday = i === lastIndex;
          return (
            <div
              key={bucket.date}
              title={`${bucket.date} · ${formatMetricValue(bucket.value, metric, t)}`}
              style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4 }}
            >
              {!dense && (
                <div style={{ fontSize: 9.5, color: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)', fontWeight: isToday ? 600 : 400 }}>
                  {formatMetricValue(bucket.value, metric, t)}
                </div>
              )}
              <div
                style={{
                  width: '100%',
                  height: `${(bucket.value / max) * 80}px`,
                  minHeight: 2,
                  borderRadius: dense ? 2 : 4,
                  background: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)',
                  opacity: bucket.value === 0 ? 0.15 : isToday ? 1 : 0.85,
                  transition: 'height 0.18s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-soft)',
                }}
              />
            </div>
          );
        })}
      </div>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--ol-ink-4)', marginTop: 8 }}>
        {dense
          ? [0, midIndex, lastIndex].map(i => <span key={i}>{shortDateLabel(buckets[i].date)}</span>)
          : buckets.map(bucket => <span key={bucket.date}>{weekDayLabel(bucket.date, t('overview.weekDays', { returnObjects: true }) as string[])}</span>)}
      </div>
    </>
  );
}

/** `YYYY-MM-DD` → `M/D`。日期键是后端按本地日历写的，直接切字符串即可，
 *  不要 new Date(key) —— 那会按 UTC 解析再转回本地，跨时区会差一天。 */
function shortDateLabel(dateKey: string): string {
  const [, month, day] = dateKey.split('-');
  return `${Number(month)}/${Number(day)}`;
}

function weekDayLabel(dateKey: string, names: string[]): string {
  const [year, month, day] = dateKey.split('-').map(Number);
  return names[new Date(year, month - 1, day).getDay()];
}

/** 条数/字数按整数千分位显示；时长转成人类可读的时/分/秒。 */
function formatMetricValue(
  value: number,
  metric: ActivityMetric,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  if (metric === 'duration') return formatLongDuration(value, t);
  return Math.round(value).toLocaleString();
}

/** 周期总时长可能是几十小时，不能沿用只处理秒/分的 formatDuration。 */
function formatLongDuration(ms: number, t: ReturnType<typeof useTranslation>['t']): string {
  if (ms <= 0) return '0';
  const totalSeconds = Math.round(ms / 1000);
  if (totalSeconds < 60) return t('common.durationSeconds', { value: totalSeconds });
  const totalMinutes = Math.floor(totalSeconds / 60);
  if (totalMinutes < 60) return t('overview.period.minutes', { value: totalMinutes });
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return t('overview.period.hoursMinutes', { hours, minutes });
}

function RecentRow({ session, modeLabel }: { session: DictationSession; modeLabel: Record<PolishMode, string> }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const onCopy = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error('clipboard unavailable');
      // 与 History 一致：润色失败/未产出时 finalText 为空，回退到识别原文，
      // 避免复制到空字符串。
      await navigator.clipboard.writeText(session.finalText.trim() ? session.finalText : session.rawTranscript);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('[overview] failed to copy recent entry', error);
    }
  };

  return (
    <div style={{ padding: '12px 18px', borderBottom: '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 12, alignItems: 'flex-start' }}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 4, minWidth: 60 }}>
        <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>
          {formatTime(session.createdAt)}
        </span>
        <Pill size="sm" tone="default">{modeLabel[session.mode]}</Pill>
      </div>
      <div style={{ flex: 1, fontSize: 12.5, color: 'var(--ol-ink-2)', whiteSpace: 'pre-line', lineHeight: 1.55, overflow: 'hidden', textOverflow: 'ellipsis', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}>
        {session.finalText.split('\n')[0]}
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 6 }}>
        <span style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
          {formatDuration(session.durationMs ?? 0, t)}
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

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const pad = (n: number) => String(n).padStart(2, '0');
  if (sameDay) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

function formatDuration(ms: number, t: ReturnType<typeof useTranslation>['t']): string {
  if (ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: sec.toFixed(1) });
  return `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`;
}
