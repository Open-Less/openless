import type { DictationSession } from '../../api/contracts';
import type { ServiceUsageRecord } from '../../api/usage';
import {
  summarizeReportedUsage,
  formatUsageNumber,
  summarizeCacheUsage,
  summarizeCacheUsageByModel,
} from '../../lib/serviceUsageMetrics';

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
  const stats: Array<[string, string, string?]> = [
    [
      text('语音识别记录', 'ASR records'),
      displayCount(asr.length),
      historyUnavailable ? undefined : String(asr.length),
    ],
    [
      text('文字处理记录', 'Text processing records'),
      formatUsageNumber(llm.length),
      String(llm.length),
    ],
    [text('多模态记录', 'Multimodal records'), displayCount(omni.length), String(omni.length)],
    [text('TTS 用量', 'TTS usage'), '—'],
    [
      text('已报告输入 Token', 'Reported input tokens'),
      formatUsageNumber(totals.input),
      totals.input?.toLocaleString(),
    ],
    [
      text('已报告输出 Token', 'Reported output tokens'),
      formatUsageNumber(totals.output),
      totals.output?.toLocaleString(),
    ],
    [
      text('缓存命中率', 'Cache hit rate'),
      cacheRate === null ? '—' : `${(cacheRate * 100).toFixed(1)}%`,
      cacheRate === null
        ? text(
            '缺少可比较的缓存数据（含旧记录）',
            'Comparable cache data unavailable, including legacy records',
          )
        : text(
            '可比较报告的输入 Token 缓存占比，按 Token 量加权',
            'Cached share of input tokens from comparable reports, weighted by token count',
          ),
    ],
    [
      text('录音时长', 'Recording duration'),
      historyUnavailable || rows.some((row) => row.durationMs == null)
        ? '—'
        : `${Math.round(seconds / 60)} ${text('分钟', 'min')}`,
    ],
  ];
  const grouped = new Map<string, number>();
  for (const row of rows) {
    const stages =
      row.pipelineMode === 'multimodal'
        ? [[text('多模态', 'Multimodal'), row.llmProvider, row.llmModel]]
        : [
            [text('语音识别', 'ASR'), row.asrProvider, row.asrModel],
            [
              row.translationActive
                ? text('翻译', 'Translation')
                : text('听写后处理', 'Dictation processing'),
              row.llmProvider,
              row.llmModel,
            ],
          ];
    for (const [stage, provider, model] of stages) {
      if (!provider) continue;
      const key = [stage, provider, model].filter(Boolean).join(' · ');
      grouped.set(key, (grouped.get(key) ?? 0) + 1);
    }
  }
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
  const maxDaily = Math.max(1, ...daily.map((day) => day.count));

  return {
    stats,
    cacheSummary,
    cacheGroups,
    cachePercent,
    grouped,
    daily,
    maxDaily,
    byPurpose,
    reported,
    historyUnavailable,
    ledgerUnavailable,
    rows: reportedRows,
  };
}
