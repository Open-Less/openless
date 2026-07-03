// Overview.tsx — 真实指标，从 listHistory + getCredentials 派生。

import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { formatComboLabel } from '../lib/hotkey';
import { getCredentials, listHistory, listActivityStats } from '../lib/ipc';
import { useMobileLayout } from '../lib/useMobileLayout';
import type { CredentialsStatus, DailyActivityStat, DictationSession, PolishMode } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';

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

const ASR_NAME_KEY_BY_ID: Record<string, string> = {
  volcengine: 'asrVolcengine',
  bailian: 'asrBailian',
  siliconflow: 'asrSiliconflow',
  zhipu: 'asrZhipu',
  groq: 'asrGroq',
  whisper: 'asrWhisper',
  openrouter: 'asrOpenrouter',
  'xiaomi-mimo-asr': 'asrXiaomiMimo',
  'foundry-local-whisper': 'asrFoundryLocalWhisper',
  'sherpa-onnx-local': 'asrSherpaOnnxLocal',
  'local-qwen3': 'asrLocalQwen3',
};

const LLM_NAME_KEY_BY_ID: Record<string, string> = {
  ark: 'ark',
  deepseek: 'deepseek',
  siliconflow: 'siliconflow',
  openai: 'openai',
  codex_oauth: 'codexOAuth',
  mimo: 'mimo',
  cometapi: 'cometapi',
  openrouterFree: 'openrouterFree',
  alibabaCoding: 'alibabaCoding',
  codingPlanX: 'codingPlanX',
  custom: 'custom',
};

type ActivityMode = 'daily' | 'weekly' | 'cumulative';

const ACTIVITY_MODES: ActivityMode[] = ['daily', 'weekly', 'cumulative'];

export function Overview({ onOpenHistory }: OverviewProps) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const modeLabel = useModeLabels();
  const [history, setHistory] = useState<DictationSession[]>([]);
  const [activityStats, setActivityStats] = useState<DailyActivityStat[]>([]);
  const [historyError, setHistoryError] = useState(false);
  const [credsError, setCredsError] = useState(false);
  const [creds, setCreds] = useState<CredentialsStatus>({
    activeAsrProvider: 'volcengine',
    activeLlmProvider: 'ark',
    asrConfigured: false,
    llmConfigured: false,
    volcengineConfigured: false,
    arkConfigured: false,
  });
  const [activityMode, setActivityMode] = useState<ActivityMode>('daily');
  const { prefs } = useHotkeySettings();
  const credentialsRequestSeq = useRef(0);

  const refreshHistory = useCallback(() => {
    setHistoryError(false);
    listHistory()
      .then(setHistory)
      .catch(error => {
        console.error('[overview] failed to load history', error);
        setHistoryError(true);
      });
  }, []);

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

  const refreshActivityStats = useCallback(() => {
    listActivityStats()
      .then(setActivityStats)
      .catch((error: unknown) => {
        console.error('[overview] failed to load activity stats', error);
      });
  }, []);

  useEffect(() => {
    refreshActivityStats();
  }, [refreshActivityStats]);

  useEffect(() => {
    refreshCredentials();
  }, [refreshCredentials, prefs?.activeAsrProvider, prefs?.activeLlmProvider]);

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
    if (activityStats.length > 0) {
      const todayStr = localDateKey(new Date());
      const stat = activityStats.find(s => s.date === todayStr);
      const segmentsToday = stat?.sessionCount ?? 0;
      const charsToday = stat?.totalChars ?? 0;
      const totalDurationMs = stat?.totalDurationMs ?? 0;
      const avgLatencyMs = segmentsToday > 0 ? totalDurationMs / segmentsToday : 0;
      return { charsToday, segmentsToday, totalDurationMs, avgLatencyMs };
    }
    // fallback: compute from history while activityStats is still loading
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const todays = history.filter(s => new Date(s.createdAt) >= today);
    const charsToday = todays.reduce((acc, s) => acc + s.finalText.length, 0);
    const segmentsToday = todays.length;
    const totalDurationMs = todays.reduce((acc, s) => acc + (s.durationMs ?? 0), 0);
    const avgLatencyMs = segmentsToday > 0 ? totalDurationMs / segmentsToday : 0;
    return { charsToday, segmentsToday, totalDurationMs, avgLatencyMs };
  }, [history, activityStats]);

  // 周历:过去 7 天每天的使用次数
  const weekly = useMemo(() => {
    const buckets = Array(7).fill(0);
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    if (activityStats.length > 0) {
      for (let i = 0; i < 7; i++) {
        const date = addDays(today, -(6 - i));
        const key = localDateKey(date);
        const stat = activityStats.find(s => s.date === key);
        buckets[i] = stat?.sessionCount ?? 0;
      }
    } else {
      // fallback: compute from history while activityStats is still loading
      history.forEach(s => {
        const d = new Date(s.createdAt);
        const diff = Math.floor((today.getTime() - d.setHours(0, 0, 0, 0)) / 86400000);
        if (diff >= 0 && diff < 7) {
          buckets[6 - diff] += 1;
        }
      });
    }
    return buckets;
  }, [history, activityStats]);

  const monthNames = useMemo(
    () => t('overview.monthNames', { returnObjects: true }) as string[],
    [t],
  );

  const yearlyActivity = useMemo(
    () => buildYearlyActivity(activityStats, history, monthNames, activityMode),
    [activityStats, history, monthNames, activityMode],
  );
  const showActivityHeatmap = prefs?.showOverviewActivityHeatmap !== false;

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
        <Metric icon="bolt" label={t('overview.metricTotal')} value={historyError ? '—' : metrics.segmentsToday.toLocaleString()} trend={
          historyError
            ? t('overview.historyLoadError')
            : prefs?.historyMaxEntries != null && metrics.segmentsToday > prefs.historyMaxEntries
              ? t('overview.metricTotalExceeded')
              : t('overview.metricTotalArchived', { count: history.length })
        } accent />
      </div>

      {/* 近 7 天和最近识别固定一个可读高度；最近识别内部滚动，避免和年度活动互相遮挡。 */}
      <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1.4fr', gap: 12, flexShrink: 0, height: mobile ? undefined : 210 }}>
        <Card padding={18} style={{ display: 'flex', flexDirection: 'column', height: mobile ? undefined : '100%', minHeight: 0, overflow: 'hidden' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>{t('overview.weekTitle')}</span>
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{t('overview.weekUnit')}</span>
          </div>
          {historyError ? (
            <div style={{ height: 100, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>
              {t('overview.historyLoadError')}
            </div>
          ) : (
            <WeekChart data={weekly} />
          )}
          <div style={{ display: 'grid', gridTemplateColumns: '28px minmax(0, 1fr)', gap: 8, fontSize: 10, color: 'var(--ol-ink-4)', marginTop: 8 }}>
            <span aria-hidden="true" />
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(7, minmax(0, 1fr))', gap: 8 }}>
              {weekDayLabels(t('overview.weekDays', { returnObjects: true }) as string[]).map((d, i) => <span key={i} style={{ textAlign: 'center' }}>{d}</span>)}
            </div>
          </div>
        </Card>

        <Card padding={0} style={{ display: 'flex', flexDirection: 'column', height: mobile ? undefined : '100%', minHeight: 0, overflow: 'hidden' }}>
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

      {showActivityHeatmap && (
        <ActivityHeatmapCard
          activity={yearlyActivity}
          mode={activityMode}
          onModeChange={setActivityMode}
          historyError={historyError}
        />
      )}
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

function WeekChart({ data }: { data: number[] }) {
  const max = Math.max(...data, 1);
  const mid = Math.ceil(max / 2);
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '28px minmax(0, 1fr)', gap: 8, height: 108 }}>
      <div style={{ display: 'flex', flexDirection: 'column', justifyContent: 'space-between', alignItems: 'flex-end', fontSize: 9.5, color: 'var(--ol-ink-4)', lineHeight: 1 }}>
        <span>{max}</span>
        <span>{mid}</span>
        <span>0</span>
      </div>
      <div style={{ position: 'relative', display: 'grid', gridTemplateColumns: 'repeat(7, minmax(0, 1fr))', alignItems: 'end', gap: 8, height: 108, paddingTop: 12, borderBottom: '0.5px solid var(--ol-line)' }}>
        <span aria-hidden="true" style={{ position: 'absolute', left: 0, right: 0, top: 12, borderTop: '0.5px solid color-mix(in srgb, var(--ol-ink) 8%, transparent)' }} />
        <span aria-hidden="true" style={{ position: 'absolute', left: 0, right: 0, top: 58, borderTop: '0.5px solid color-mix(in srgb, var(--ol-ink) 6%, transparent)' }} />
        {data.map((v, i) => {
          const isToday = i === 6;
          const barHeight = v > 0 ? Math.max(6, (v / max) * 74) : 3;
          return (
            <div key={i} style={{ position: 'relative', zIndex: 1, flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'flex-end', gap: 4 }}>
              <div style={{ fontSize: 9.5, color: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)', fontWeight: isToday ? 700 : 500 }}>{v}</div>
              <div
                style={{
                  width: '100%',
                  height: barHeight,
                  borderRadius: '4px 4px 1px 1px',
                  background: isToday ? 'var(--ol-blue)' : 'color-mix(in srgb, var(--ol-blue) 58%, var(--ol-surface))',
                  opacity: v === 0 ? 0.22 : 1,
                  transition: 'height 0.18s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-soft)',
                }}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

interface DayActivity {
  key: string;
  date: Date;
  rawCount: number;
  rawChars: number;
  rawDurationMs: number;
  count: number;
  chars: number;
  durationMs: number;
  level: number;
  inRange: boolean;
}

interface WeekActivity {
  key: string;
  startDate: Date;
  endDate: Date;
  count: number;
  chars: number;
  durationMs: number;
  cumulativeCount: number;
  cumulativeChars: number;
  cumulativeDurationMs: number;
  value: number;
  valueChars: number;
  level: number;
  inRange: boolean;
}

interface MonthLabel {
  weekIndex: number;
  label: string;
}

interface YearlyActivity {
  cells: DayActivity[];
  weeks: number;
  weekColumns: number;
  activeDays: number;
  maxCount: number;
  maxWeekValue: number;
  weekBars: WeekActivity[];
  monthLabels: MonthLabel[];
  weekMonthLabels: MonthLabel[];
}

function ActivityHeatmapCard({
  activity,
  mode,
  onModeChange,
  historyError,
}: {
  activity: YearlyActivity;
  mode: ActivityMode;
  onModeChange: (mode: ActivityMode) => void;
  historyError: boolean;
}) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const cardRef = useRef<HTMLDivElement | null>(null);
  const [hoveredKey, setHoveredKey] = useState<string | null>(null);
  const [hoverPoint, setHoverPoint] = useState<{ x: number; y: number; align: 'left' | 'center' | 'right' } | null>(null);
  const cellSize = 12;
  const cellGap = 4;
  const dayGridWidth = activity.weeks * cellSize + Math.max(0, activity.weeks - 1) * cellGap;
  const weekGridWidth = activity.weekColumns * cellSize + Math.max(0, activity.weekColumns - 1) * cellGap;
  const hoveredDay = mode === 'daily' && hoveredKey
    ? activity.cells.find(cell => cell.key === hoveredKey) ?? null
    : null;
  const hoveredWeek = mode !== 'daily' && hoveredKey
    ? activity.weekBars.find(week => week.key === hoveredKey) ?? null
    : null;
  const summaryArgs = hoveredDay
    ? activityDaySummaryArgs(hoveredDay, t)
    : hoveredWeek && mode !== 'daily'
      ? activityWeekSummaryArgs(mode, hoveredWeek, t)
      : null;
  const summaryText = summaryArgs ? t(summaryArgs.key, summaryArgs.options) : null;
  const showTooltip = Boolean(summaryText && hoverPoint);
  const handleHover = useCallback((key: string, event: PointerEvent<HTMLElement>) => {
    const cardRect = cardRef.current?.getBoundingClientRect();
    const targetRect = event.currentTarget.getBoundingClientRect();
    if (!cardRect) return;
    const x = targetRect.left + targetRect.width / 2 - cardRect.left;
    const y = targetRect.top - cardRect.top;
    const align = x < 140 ? 'left' : x > cardRect.width - 140 ? 'right' : 'center';
    setHoveredKey(key);
    setHoverPoint({ x, y, align });
  }, []);
  const clearHover = useCallback(() => {
    setHoveredKey(null);
    setHoverPoint(null);
  }, []);

  return (
    <Card padding={18} style={{ marginTop: 12, overflow: 'hidden', flexShrink: 0, position: 'relative' }}>
      <div ref={cardRef} style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }} />
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: mobile ? '1fr' : 'minmax(0, 1fr) auto',
          alignItems: 'center',
          gap: mobile ? 10 : 14,
          marginBottom: 16,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0, justifySelf: 'start' }}>
          <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>{t('overview.activityTitle')}</span>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: mobile ? 'flex-start' : 'flex-end', gap: 4, justifySelf: mobile ? 'start' : 'end' }}>
          {ACTIVITY_MODES.map(activityMode => {
            const selected = activityMode === mode;
            return (
              <button
                key={activityMode}
                type="button"
                onClick={() => onModeChange(activityMode)}
                aria-pressed={selected}
                style={{
                  height: 28,
                  padding: '0 8px',
                  border: '0.5px solid transparent',
                  borderRadius: 7,
                  background: selected ? 'color-mix(in srgb, var(--ol-ink) 8%, transparent)' : 'transparent',
                  color: selected ? 'var(--ol-ink)' : 'var(--ol-ink-4)',
                  fontSize: 12,
                  fontWeight: selected ? 700 : 600,
                  cursor: 'pointer',
                  transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
                }}
              >
                {t(`overview.activityMode.${activityMode}`)}
              </button>
            );
          })}
        </div>
      </div>

      {historyError ? (
        <div style={{ minHeight: 116, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('overview.historyLoadError')}</div>
      ) : activity.activeDays === 0 ? (
        <div style={{ minHeight: 116, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('overview.activityEmpty')}</div>
      ) : mode === 'daily' ? (
        <ActivityDailyGrid
          mode={mode}
          activity={activity}
          cellSize={cellSize}
          cellGap={cellGap}
          minGridWidth={dayGridWidth}
          hoveredKey={hoveredKey}
          onHover={handleHover}
          onLeave={clearHover}
        />
      ) : (
        <ActivityWeekGrid
          activity={activity}
          mode={mode}
          cellSize={cellSize}
          cellGap={cellGap}
          minGridWidth={weekGridWidth}
          hoveredKey={hoveredKey}
          onHover={handleHover}
          onLeave={clearHover}
        />
      )}
      {showTooltip && hoverPoint && (
        <div
          style={{
            position: 'absolute',
            left: hoverPoint.x,
            top: Math.max(34, hoverPoint.y - 3),
            transform: hoverPoint.align === 'left'
              ? 'translate(0, -100%)'
              : hoverPoint.align === 'right'
                ? 'translate(-100%, -100%)'
                : 'translate(-50%, -100%)',
            zIndex: 2,
            maxWidth: 260,
            padding: '8px 12px',
            border: '0.5px solid color-mix(in srgb, var(--ol-ink) 10%, transparent)',
            borderRadius: 8,
            background: 'var(--ol-surface)',
            color: 'var(--ol-ink)',
            boxShadow: '0 8px 24px color-mix(in srgb, var(--ol-ink) 12%, transparent)',
            fontSize: 11.5,
            fontWeight: 500,
            lineHeight: 1.4,
            pointerEvents: 'none',
          }}
        >
          {summaryText}
        </div>
      )}
    </Card>
  );
}

function ActivityDailyGrid({
  mode,
  activity,
  cellSize,
  cellGap,
  minGridWidth,
  hoveredKey,
  onHover,
  onLeave,
}: {
  mode: ActivityMode;
  activity: YearlyActivity;
  cellSize: number;
  cellGap: number;
  minGridWidth: number;
  hoveredKey: string | null;
  onHover: (key: string, event: PointerEvent<HTMLElement>) => void;
  onLeave: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div onPointerLeave={onLeave}>
      <div className="ol-thinscroll" style={{ overflowX: 'auto', overflowY: 'hidden', paddingBottom: 2, flex: 1 }}>
        <div style={{ minWidth: minGridWidth, width: 'max-content' }}>
          <div
            aria-label={t('overview.activityTitle')}
            style={{
              display: 'grid',
              gridTemplateColumns: `repeat(${activity.weeks}, ${cellSize}px)`,
              gridTemplateRows: `repeat(7, ${cellSize}px)`,
              gridAutoFlow: 'column',
              gap: cellGap,
            }}
          >
            {activity.cells.map(cell => {
              const selected = hoveredKey === cell.key;
              const cellSummary = activityDaySummaryArgs(cell, t);
              const weekIndex = Math.floor(differenceInDays(cell.date, activity.cells[0]?.date ?? cell.date) / 7);
              return (
                <div
                  key={`${mode}-${cell.key}`}
                  title={cell.inRange ? t(cellSummary.key, cellSummary.options) : undefined}
                  onPointerEnter={(event) => {
                    if (cell.inRange) onHover(cell.key, event);
                  }}
                  onPointerMove={(event) => {
                    if (cell.inRange) onHover(cell.key, event);
                  }}
                  style={{
                    width: cellSize,
                    height: cellSize,
                    borderRadius: 4,
                    background: activityColor(cell.level),
                    opacity: cell.inRange ? 1 : 0,
                    boxShadow: cell.inRange
                      ? selected
                        ? '0 0 0 1.5px color-mix(in srgb, var(--ol-ink) 42%, transparent) inset'
                        : '0 0 0 0.5px color-mix(in srgb, var(--ol-ink) 10%, transparent) inset'
                      : 'none',
                    cursor: cell.inRange ? 'default' : 'auto',
                    transform: 'scale(1)',
                    animation: cell.inRange ? `ol-activity-cell-in 0.22s var(--ol-motion-soft) ${Math.min(Math.max(0, weekIndex) * 5, 260)}ms both` : undefined,
                    transition: 'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick), box-shadow 0.12s var(--ol-motion-quick)',
                  }}
                />
              );
            })}
          </div>
          <ActivityMonthLabels labels={activity.monthLabels} weeks={activity.weeks} cellSize={cellSize} cellGap={cellGap} />
        </div>
      </div>
    </div>
  );
}

function ActivityWeekGrid({
  activity,
  mode,
  cellSize,
  cellGap,
  minGridWidth,
  hoveredKey,
  onHover,
  onLeave,
}: {
  activity: YearlyActivity;
  mode: Exclude<ActivityMode, 'daily'>;
  cellSize: number;
  cellGap: number;
  minGridWidth: number;
  hoveredKey: string | null;
  onHover: (key: string, event: PointerEvent<HTMLElement>) => void;
  onLeave: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div onPointerLeave={onLeave}>
      <div className="ol-thinscroll" style={{ overflowX: 'auto', overflowY: 'hidden', paddingBottom: 2, flex: 1 }}>
      <div style={{ minWidth: minGridWidth, width: 'max-content' }}>
        <div
          aria-label={t(`overview.activityMode.${mode}`)}
          style={{
            display: 'grid',
            gridTemplateColumns: `repeat(${activity.weekColumns}, ${cellSize}px)`,
            gridTemplateRows: `repeat(7, ${cellSize}px)`,
            gridAutoFlow: 'column',
            gap: cellGap,
          }}
        >
          {activity.weekBars.flatMap((week, weekIndex) => {
            const summary = activityWeekSummaryArgs(mode, week, t);
            const filledCells = activityDiscreteCells(week.value, activity.maxWeekValue);
            return Array.from({ length: 7 }).map((_, rowIndex) => {
              const selected = hoveredKey === week.key;
              const filled = rowIndex >= 7 - filledCells;
              return (
              <div
                key={`${mode}-${week.key}-${rowIndex}`}
                title={week.inRange ? t(summary.key, summary.options) : undefined}
                onPointerEnter={(event) => {
                  if (week.inRange) onHover(week.key, event);
                }}
                onPointerMove={(event) => {
                  if (week.inRange) onHover(week.key, event);
                }}
                style={{
                  width: cellSize,
                  height: cellSize,
                  borderRadius: 4,
                  background: filled ? activityColor(week.level) : activityColor(0),
                  opacity: week.inRange ? 1 : 0,
                  boxShadow: week.inRange
                    ? selected
                      ? '0 0 0 1.5px color-mix(in srgb, var(--ol-ink) 42%, transparent) inset'
                      : '0 0 0 0.5px color-mix(in srgb, var(--ol-ink) 10%, transparent) inset'
                    : 'none',
                  cursor: week.inRange ? 'default' : 'auto',
                  transform: 'scale(1)',
                  animation: week.inRange ? `ol-activity-cell-in 0.22s var(--ol-motion-soft) ${Math.min(weekIndex * 5, 260)}ms both` : undefined,
                  transition: 'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick), box-shadow 0.12s var(--ol-motion-quick)',
                }}
              />
              );
            });
          })}
        </div>
        <ActivityMonthLabels labels={activity.weekMonthLabels} weeks={activity.weekColumns} cellSize={cellSize} cellGap={cellGap} />
      </div>
      </div>
    </div>
  );
}

function ActivityMonthLabels({
  labels,
  weeks,
  cellSize,
  cellGap,
}: {
  labels: MonthLabel[];
  weeks: number;
  cellSize: number;
  cellGap: number;
}) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: `repeat(${weeks}, ${cellSize}px)`,
        columnGap: cellGap,
        minHeight: 18,
        marginTop: 12,
        color: 'var(--ol-ink-4)',
        fontSize: 11,
      }}
    >
      {labels.map(label => (
        <span
          key={`${label.weekIndex}-${label.label}`}
          style={{
            gridColumn: `${label.weekIndex + 1}`,
            whiteSpace: 'nowrap',
            overflow: 'visible',
            lineHeight: '16px',
          }}
        >
          {label.label}
        </span>
      ))}
    </div>
  );
}

function RecentRow({ session, modeLabel }: { session: DictationSession; modeLabel: Record<PolishMode, string> }) {
  const { t } = useTranslation();
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
      <span style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
        {formatDuration(session.durationMs ?? 0, t)}
      </span>
    </div>
  );
}

function buildYearlyActivity(
  dailyStats: DailyActivityStat[],
  initialHistory: DictationSession[],
  monthNames: string[],
  mode: ActivityMode,
): YearlyActivity {
  const today = startOfLocalDay(new Date());
  const rangeStart = addDays(today, -364);
  const gridStart = rangeStart;
  const weeks = Math.ceil((differenceInDays(today, gridStart) + 1) / 7);
  const weekGridStart = startOfWeek(rangeStart);
  const weekColumns = Math.ceil((differenceInDays(today, weekGridStart) + 1) / 7);
  const byDay = new Map<string, { count: number; chars: number; durationMs: number }>();

  const todayStr = localDateKey(today);
  if (dailyStats.length > 0) {
    for (const stat of dailyStats) {
      if (stat.date < localDateKey(rangeStart) || stat.date > todayStr) continue;
      byDay.set(stat.date, {
        count: stat.sessionCount,
        chars: stat.totalChars,
        durationMs: stat.totalDurationMs,
      });
    }
  } else {
    // fallback: iterate history while activityStats is still loading
    initialHistory.forEach(session => {
      const date = startOfLocalDay(new Date(session.createdAt));
      if (isNaN(date.getTime()) || date < rangeStart || date > today) return;
      const key = localDateKey(date);
      const current = byDay.get(key) ?? { count: 0, chars: 0, durationMs: 0 };
      current.count += 1;
      current.chars += session.finalText.length;
      current.durationMs += session.durationMs ?? 0;
      byDay.set(key, current);
    });
  }

  const rawCells: Array<Omit<DayActivity, 'count' | 'chars' | 'durationMs' | 'level'>> = [];
  for (let i = 0; i < weeks * 7; i += 1) {
    const date = addDays(gridStart, i);
    const inRange = date >= rangeStart && date <= today;
    const key = localDateKey(date);
    const stats = inRange ? byDay.get(key) : undefined;
    rawCells.push({
      key,
      date,
      rawCount: stats?.count ?? 0,
      rawChars: stats?.chars ?? 0,
      rawDurationMs: stats?.durationMs ?? 0,
      inRange,
    });
  }

  let cumulativeCount = 0;
  let cumulativeChars = 0;
  let cumulativeDurationMs = 0;
  const metricCells = rawCells.map(cell => ({
    ...cell,
    count: cell.inRange ? cell.rawCount : 0,
    chars: cell.inRange ? cell.rawChars : 0,
    durationMs: cell.inRange ? cell.rawDurationMs : 0,
  }));

  const maxCount = Math.max(...metricCells.filter(cell => cell.inRange).map(cell => cell.count), 0);
  const cells = metricCells.map(cell => ({
    ...cell,
    level: cell.inRange ? activityLevel(cell.count, maxCount) : 0,
  }));

  const rawWeeks: Array<Omit<WeekActivity, 'level'>> = [];
  for (let weekIndex = 0; weekIndex < weekColumns; weekIndex += 1) {
    const startDate = addDays(weekGridStart, weekIndex * 7);
    const endDate = addDays(startDate, 6);
    const inRange = endDate >= rangeStart && startDate <= today;
    let stats = { count: 0, chars: 0, durationMs: 0 };
    for (let offset = 0; offset < 7; offset += 1) {
      const date = addDays(startDate, offset);
      if (date < rangeStart || date > today) continue;
      const dayStats = byDay.get(localDateKey(date));
      if (!dayStats) continue;
      stats = {
        count: stats.count + dayStats.count,
        chars: stats.chars + dayStats.chars,
        durationMs: stats.durationMs + dayStats.durationMs,
      };
    }
    if (inRange) {
      cumulativeCount += stats.count;
      cumulativeChars += stats.chars;
      cumulativeDurationMs += stats.durationMs;
    }
    const value = mode === 'cumulative' ? cumulativeCount : stats.count;
    const valueChars = mode === 'cumulative' ? cumulativeChars : stats.chars;
    rawWeeks.push({
      key: localDateKey(startDate),
      startDate,
      endDate: endDate > today ? today : endDate,
      count: inRange ? stats.count : 0,
      chars: inRange ? stats.chars : 0,
      durationMs: inRange ? stats.durationMs : 0,
      cumulativeCount: inRange ? cumulativeCount : 0,
      cumulativeChars: inRange ? cumulativeChars : 0,
      cumulativeDurationMs: inRange ? cumulativeDurationMs : 0,
      value: inRange ? value : 0,
      valueChars: inRange ? valueChars : 0,
      inRange,
    });
  }

  const maxWeekValue = Math.max(...rawWeeks.filter(week => week.inRange).map(week => week.value), 0);
  const weekBars = rawWeeks.map(week => ({
    ...week,
    level: week.inRange ? activityLevel(week.value, maxWeekValue) : 0,
  }));

  // 收集范围内每个月的起始/结束列 → 取中间列作为标签位置，使月份标签均匀分布
  const monthRanges = new Map<number, { firstCol: number; lastCol: number }>();
  for (let weekIndex = 0; weekIndex < weekColumns; weekIndex += 1) {
    const weekStart = addDays(weekGridStart, weekIndex * 7);
    for (let offset = 0; offset < 7; offset += 1) {
      const date = addDays(weekStart, offset);
      if (date < rangeStart || date > today) continue;
      const month = date.getMonth();
      const range = monthRanges.get(month);
      if (range) {
        range.firstCol = Math.min(range.firstCol, weekIndex);
        range.lastCol = Math.max(range.lastCol, weekIndex);
      } else {
        monthRanges.set(month, { firstCol: weekIndex, lastCol: weekIndex });
      }
    }
  }
  const monthLabels: MonthLabel[] = [];
  const weekMonthLabels: MonthLabel[] = [];
  for (const [month, { firstCol, lastCol }] of monthRanges) {
    const label = monthNames[month] ?? String(month + 1);
    const midCol = Math.floor((firstCol + lastCol) / 2);
    if (midCol < weeks) {
      monthLabels.push({ weekIndex: midCol, label });
    }
    weekMonthLabels.push({ weekIndex: midCol, label });
  }

  return {
    cells,
    weeks,
    weekColumns,
    activeDays: rawCells.filter(cell => cell.inRange && cell.rawCount > 0).length,
    maxCount,
    maxWeekValue,
    weekBars,
    monthLabels,
    weekMonthLabels,
  };
}

function activityLevel(count: number, maxCount: number): number {
  if (count <= 0 || maxCount <= 0) return 0;
  const ratio = count / maxCount;
  if (ratio <= 0.25) return 1;
  if (ratio <= 0.5) return 2;
  if (ratio <= 0.75) return 3;
  return 4;
}

function activityDiscreteCells(value: number, maxValue: number): number {
  if (value <= 0 || maxValue <= 0) return 0;
  return Math.min(7, Math.max(1, Math.ceil((value / maxValue) * 7)));
}

function activityColor(level: number): string {
  const base = 'var(--ol-blue)';
  switch (level) {
    case 1:
      return `color-mix(in srgb, ${base} 26%, var(--ol-surface))`;
    case 2:
      return `color-mix(in srgb, ${base} 44%, var(--ol-surface))`;
    case 3:
      return `color-mix(in srgb, ${base} 68%, var(--ol-surface))`;
    case 4:
      return base;
    default:
      return 'color-mix(in srgb, var(--ol-ink) 8%, var(--ol-surface))';
  }
}

function activityDaySummaryArgs(cell: DayActivity, t: ReturnType<typeof useTranslation>['t']): { key: string; options: Record<string, string | number> } {
  const date = formatHeatmapMonthDay(cell.date);
  return {
    key: 'overview.activitySummaryDaily',
    options: {
      date,
      count: cell.rawCount.toLocaleString(),
      chars: cell.rawChars.toLocaleString(),
      duration: formatDuration(cell.rawDurationMs, t),
    },
  };
}

function activityWeekSummaryArgs(mode: Exclude<ActivityMode, 'daily'>, week: WeekActivity, t: ReturnType<typeof useTranslation>['t']): { key: string; options: Record<string, string | number> } {
  const start = formatHeatmapDisplayDate(week.startDate);
  if (mode === 'weekly') {
    return {
      key: 'overview.activitySummaryWeekly',
      options: {
        start,
        count: week.count.toLocaleString(),
        chars: week.chars.toLocaleString(),
        duration: formatDuration(week.durationMs, t),
      },
    };
  }
  return {
    key: 'overview.activitySummaryCumulative',
    options: {
      start,
      count: week.cumulativeCount.toLocaleString(),
      chars: week.cumulativeChars.toLocaleString(),
      duration: formatDuration(week.cumulativeDurationMs, t),
    },
  };
}

function startOfLocalDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function startOfWeek(date: Date): Date {
  return addDays(startOfLocalDay(date), -((date.getDay() + 6) % 7));
}

function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

function differenceInDays(a: Date, b: Date): number {
  return Math.round((startOfLocalDay(a).getTime() - startOfLocalDay(b).getTime()) / 86400000);
}

function localDateKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function formatHeatmapDisplayDate(date: Date): string {
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' });
}

function formatHeatmapMonthDay(date: Date): string {
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' });
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
  if (ms <= 0) return t('common.durationSeconds', { value: '0' });
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: sec.toFixed(1) });
  if (sec < 3600) return t('common.durationMinutes', { value: (sec / 60).toFixed(1) });
  return t('common.durationHours', { value: (sec / 3600).toFixed(1) });
}

function weekDayLabels(names: string[]): string[] {
  const today = new Date().getDay();
  const out: string[] = [];
  for (let i = 6; i >= 0; i--) {
    out.push(names[(today - i + 7) % 7]);
  }
  return out;
}
