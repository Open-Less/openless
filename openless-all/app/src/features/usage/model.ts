import type { DictationSession } from '../../api/contracts';
import type { ServiceUsageRecord } from '../../api/usage';
import {
  summarizeReportedUsage,
  formatUsageNumber,
  summarizeCacheUsage,
  summarizeCacheUsageByModel,
} from '../../lib/serviceUsageMetrics';

export type UsageStageKind = 'asr' | 'postprocess' | 'translation' | 'multimodal';

export interface UsageMetricItem {
  id: string;
  icon: string;
  label: string;
  value: string;
  exact?: string;
  empty: boolean;
  badge?: string;
  badgeTitle?: string;
}

export interface UsageServiceItem {
  key: string;
  stage: string;
  stageKind: UsageStageKind;
  provider: string;
  model: string | null;
  count: number;
  share: number;
}

export function buildUsageDashboard(
  rows: DictationSession[] | null,
  reports: ServiceUsageRecord[] | null,
  days: number,
  text: (cn: string, en: string) => string,
  now = Date.now(),
) {
  const historyUnavailable = rows === null;
  const ledgerUnavailable = reports === null;
  const reportedRows = rows ?? [];
  rows = reportedRows;
  const displayCount = (count: number) => (historyUnavailable ? '—' : formatUsageNumber(count));
  const asr = rows.filter((row) => row.pipelineMode !== 'multimodal' && row.asrProvider);
  const llm = rows.filter((row) => row.pipelineMode !== 'multimodal' && row.llmProvider);
  const omni = rows.filter((row) => row.pipelineMode === 'multimodal' && row.llmProvider);
  const {
    rows: reported,
    totals,
    byPurpose,
  } = summarizeReportedUsage(reports ?? [], now - days * 86_400_000, now);
  const cacheSummary = summarizeCacheUsage(reported);
  const cacheRate = cacheSummary.rate;
  const cacheGroups = summarizeCacheUsageByModel(reported);
  const cachePercent = (rate: number | null) =>
    rate === null ? '—' : `${(rate * 100).toFixed(1)}%`;
  const seconds = rows.reduce((total, row) => total + (row.durationMs ?? 0), 0) / 1000;
  const durationDisplay =
    historyUnavailable || rows.some((row) => row.durationMs == null)
      ? '—'
      : seconds > 0 && seconds < 60
        ? `${Math.round(seconds)} ${text('秒', 'sec')}`
        : `${Math.round(seconds / 60)} ${text('分钟', 'min')}`;
  const cacheRateText = cacheRate === null ? '—' : `${(cacheRate * 100).toFixed(1)}%`;
  const cacheRateTitle =
    cacheRate === null
      ? text(
          '缺少可比较的缓存数据（含旧记录）',
          'Comparable cache data unavailable, including legacy records',
        )
      : text(
          '可比较报告的输入 Token 缓存占比，按 Token 量加权',
          'Cached share of input tokens from comparable reports, weighted by token count',
        );
  const metricItems: UsageMetricItem[] = [
    {
      id: 'asr',
      icon: 'mic',
      label: text('语音识别记录', 'ASR records'),
      value: displayCount(asr.length),
      exact: historyUnavailable ? undefined : String(asr.length),
      empty: historyUnavailable,
    },
    {
      id: 'llm',
      icon: 'sparkle',
      label: text('文字处理记录', 'Text processing records'),
      value: formatUsageNumber(llm.length),
      exact: String(llm.length),
      empty: false,
    },
    {
      id: 'multimodal',
      icon: 'layout',
      label: text('多模态记录', 'Multimodal records'),
      value: displayCount(omni.length),
      exact: String(omni.length),
      empty: historyUnavailable,
    },
    {
      id: 'tts',
      icon: 'play',
      label: text('TTS 用量', 'TTS usage'),
      value: '—',
      empty: true,
    },
    {
      id: 'inputTokens',
      icon: 'download',
      label: text('已报告输入 Token', 'Reported input tokens'),
      value: formatUsageNumber(totals.input),
      exact: totals.input?.toLocaleString(),
      empty: totals.input === null,
    },
    {
      id: 'outputTokens',
      icon: 'upload',
      label: text('已报告输出 Token', 'Reported output tokens'),
      value: formatUsageNumber(totals.output),
      exact: totals.output?.toLocaleString(),
      empty: totals.output === null,
    },
    {
      id: 'cacheRate',
      icon: 'bolt',
      label: text('缓存命中率', 'Cache hit rate'),
      value: cacheRateText,
      exact: cacheRateTitle,
      empty: cacheRate === null,
      badge: ledgerUnavailable
        ? undefined
        : `${cacheSummary.comparableRecords}/${cacheSummary.totalRecords}`,
      badgeTitle: text('可比较报告 / 已保存报告', 'Comparable reports / saved reports'),
    },
    {
      id: 'duration',
      icon: 'clock',
      label: text('录音时长', 'Recording duration'),
      value: durationDisplay,
      exact:
        durationDisplay === '—' ? undefined : `${Math.round(seconds)} ${text('秒', 'seconds')}`,
      empty: durationDisplay === '—',
    },
  ];
  const stats: Array<[string, string, string?]> = metricItems.map((item) => [
    item.label,
    item.value,
    item.exact,
  ]);

  const grouped = new Map<string, number>();
  const serviceMap = new Map<
    string,
    {
      stage: string;
      stageKind: UsageStageKind;
      provider: string;
      model: string | null;
      count: number;
    }
  >();
  for (const row of rows) {
    const stages: Array<
      [string, UsageStageKind, string | null | undefined, string | null | undefined]
    > =
      row.pipelineMode === 'multimodal'
        ? [[text('多模态', 'Multimodal'), 'multimodal', row.llmProvider, row.llmModel]]
        : [
            [text('语音识别', 'ASR'), 'asr', row.asrProvider, row.asrModel],
            [
              row.translationActive
                ? text('翻译', 'Translation')
                : text('听写后处理', 'Dictation processing'),
              row.translationActive ? 'translation' : 'postprocess',
              row.llmProvider,
              row.llmModel,
            ],
          ];
    for (const [stage, stageKind, provider, model] of stages) {
      if (!provider) continue;
      const key = [stage, provider, model].filter(Boolean).join(' · ');
      grouped.set(key, (grouped.get(key) ?? 0) + 1);
      const existing = serviceMap.get(key);
      if (existing) {
        existing.count += 1;
      } else {
        serviceMap.set(key, {
          stage,
          stageKind,
          provider,
          model: model || null,
          count: 1,
        });
      }
    }
  }
  const totalServiceCalls = [...serviceMap.values()].reduce((sum, item) => sum + item.count, 0);
  const serviceItems: UsageServiceItem[] = [...serviceMap.entries()]
    .map(([key, item]) => ({
      key,
      ...item,
      share: totalServiceCalls > 0 ? item.count / totalServiceCalls : 0,
    }))
    .sort((a, b) => b.count - a.count || a.key.localeCompare(b.key));

  const daily = Array.from({ length: days }, (_, index) => {
    const date = new Date(now);
    date.setHours(0, 0, 0, 0);
    date.setDate(date.getDate() - (days - index - 1));
    const end = new Date(date);
    end.setDate(end.getDate() + 1);
    return {
      date,
      count: rows.filter((row) => {
        const time = new Date(row.createdAt).getTime();
        return time >= date.getTime() && time < end.getTime();
      }).length,
    };
  });
  const totalDaily = daily.reduce((sum, day) => sum + day.count, 0);
  const maxDaily = Math.max(1, ...daily.map((day) => day.count));

  return {
    days,
    stats,
    metricItems,
    cacheSummary,
    cacheGroups,
    cachePercent,
    grouped,
    serviceItems,
    totalServiceCalls,
    daily,
    totalDaily,
    maxDaily,
    byPurpose,
    reported,
    historyUnavailable,
    ledgerUnavailable,
    rows: reportedRows,
  };
}
