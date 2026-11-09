import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listHistory } from '../lib/ipc/history';
import {
  clearServiceUsage,
  getServiceUsage,
  type ServiceUsageSnapshot,
} from '../lib/ipc/service-usage';
import type { DictationSession } from '../lib/types';
import { summarizeReportedUsage } from '../lib/serviceUsageMetrics';
import { Btn, Card, PageHeader } from './_atoms';

export function Usage() {
  const { i18n } = useTranslation();
  const zh = i18n.resolvedLanguage?.startsWith('zh') ?? false;
  const text = (cn: string, en: string) => (zh ? cn : en);
  const [history, setHistory] = useState<DictationSession[] | null>(null);
  const [error, setError] = useState(false);
  const [days, setDays] = useState(30);
  const [usage, setUsage] = useState<ServiceUsageSnapshot | null>(null);
  const [usageError, setUsageError] = useState(false);
  const [clearError, setClearError] = useState(false);
  const [clearing, setClearing] = useState(false);
  const mounted = useRef(false);
  const clearingRef = useRef(false);
  const request = useRef(0);
  const refresh = useCallback(async () => {
    if (clearingRef.current) return;
    const id = ++request.current;
    const [historyResult, usageResult] = await Promise.allSettled([
      listHistory(),
      getServiceUsage(),
    ]);
    if (!mounted.current || request.current !== id) return;
    if (historyResult.status === 'fulfilled') {
      setHistory(historyResult.value);
      setError(false);
    } else {
      setHistory(null);
      setError(true);
    }
    if (usageResult.status === 'fulfilled') {
      setUsage(usageResult.value);
      setUsageError(false);
      setClearError(false);
    } else {
      setUsage(null);
      setUsageError(true);
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener('focus', onFocus);
    return () => {
      mounted.current = false;
      request.current += 1;
      window.removeEventListener('focus', onFocus);
    };
  }, [refresh]);
  const rows = useMemo(() => {
    const cutoff = Date.now() - days * 86_400_000;
    return (history ?? []).filter(
      (row) =>
        (!row.source || row.source === 'voice') && new Date(row.createdAt).getTime() >= cutoff,
    );
  }, [history, days]);
  const asr = rows.filter((row) => row.pipelineMode !== 'multimodal' && row.asrProvider);
  const llm = rows.filter((row) => row.pipelineMode !== 'multimodal' && row.llmProvider);
  const omni = rows.filter((row) => row.pipelineMode === 'multimodal' && row.llmProvider);
  const now = Date.now();
  const {
    rows: reported,
    totals,
    byPurpose,
  } = summarizeReportedUsage(usage?.records ?? [], now - days * 86_400_000, now);
  const purposeName = (purpose: string) => {
    const labels: Record<string, [string, string]> = {
      dictationPostprocess: ['听写后处理', 'Dictation processing'],
      translation: ['翻译', 'Translation'],
      selection: ['选区处理', 'Selection'],
      question: ['问答', 'Questions'],
      quickNote: ['速记', 'Quick note'],
      voiceEdit: ['语音编辑', 'Voice editing'],
      cloudNote: ['云笔记', 'Cloud note'],
    };
    const label = labels[purpose];
    return label ? text(...label) : text('其他', 'Other');
  };
  const safeTokens = (value: number | null) =>
    value !== null && Number.isSafeInteger(value) && value >= 0 ? value.toLocaleString() : '—';
  const seconds = rows.reduce((total, row) => total + (row.durationMs ?? 0), 0) / 1000;
  const stats = [
    [text('语音识别记录', 'ASR records'), asr.length],
    [text('文字处理记录', 'Text processing records'), llm.length],
    [text('多模态记录', 'Multimodal records'), omni.length],
    [text('TTS 用量', 'TTS usage'), '—'],
    [text('已报告输入 Token', 'Reported input tokens'), totals.input?.toLocaleString() ?? '—'],
    [text('已报告输出 Token', 'Reported output tokens'), totals.output?.toLocaleString() ?? '—'],
    [
      text('录音时长', 'Recording duration'),
      rows.some((row) => row.durationMs == null)
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
    const date = new Date();
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
  return (
    <>
      <PageHeader
        title={text('用量', 'Usage')}
        desc={text(
          '按服务与用途查看本机保留的使用记录。',
          'Review retained local records by service and purpose.',
        )}
        right={
          <Btn icon="refresh" disabled={clearing} onClick={() => void refresh()}>
            {text('刷新', 'Refresh')}
          </Btn>
        }
      />
      <p style={{ color: 'var(--ol-ink-3)', fontSize: 13, lineHeight: 1.6 }}>
        {text(
          '记录数仅来自本机保留的听写历史（最多 200 条）。Token 仅包含桌面端兼容接口明确返回并成功保存的部分；其他调用、TTS 和费用尚未完整记录，不能作为账单。',
          'Record counts come only from retained dictation history (up to 200 entries). Tokens include only provider-reported values saved from supported desktop calls; other calls, TTS and costs are incomplete and are not a bill.',
        )}
      </p>
      <div style={{ display: 'flex', gap: 8, margin: '8px 0 16px' }}>
        {[7, 30].map((day) => (
          <button
            key={day}
            type="button"
            className="ol-pressable"
            aria-pressed={days === day}
            onClick={() => setDays(day)}
            style={{
              padding: '7px 12px',
              border: 0,
              borderRadius: 8,
              color: days === day ? 'white' : 'var(--ol-ink-2)',
              background: days === day ? 'var(--ol-blue)' : 'var(--ol-surface-2)',
            }}
          >
            {text(`近 ${day} 天`, `Last ${day} days`)}
          </button>
        ))}
      </div>
      {usageError && (
        <p role="alert">
          {text('无法读取本地 Token 账本。', 'Could not read the local token ledger.')}
        </p>
      )}
      {clearError && (
        <p role="alert">
          {text(
            '清除结果未确认，请刷新后检查。',
            'The clear result is unconfirmed. Refresh to check.',
          )}
        </p>
      )}
      {usage && usage.droppedOrFailed > 0 && (
        <p role="alert">
          {text(
            '本次运行中部分用量未能保存，统计不完整。',
            'Some usage could not be saved during this run.',
          )}
        </p>
      )}
      {usage && (
        <button
          type="button"
          className="ol-pressable"
          disabled={clearing}
          style={{ alignSelf: 'flex-start', borderRadius: 8, padding: '7px 12px' }}
          onClick={async () => {
            clearingRef.current = true;
            request.current += 1;
            setClearing(true);
            setClearError(false);
            try {
              await clearServiceUsage();
              if (mounted.current)
                setUsage({ records: [], droppedOrFailed: usage.droppedOrFailed });
            } catch {
              if (mounted.current) setClearError(true);
            } finally {
              clearingRef.current = false;
              if (mounted.current) setClearing(false);
            }
          }}
        >
          {text('清除已记录 Token', 'Clear recorded tokens')}
        </button>
      )}
      {usage && (
        <p style={{ color: 'var(--ol-ink-3)', fontSize: 12 }}>
          {text(
            '清除前或清除期间已开始的请求可能不会计入新账本。',
            'Requests started before or during clearing may not appear in the new ledger.',
          )}
        </p>
      )}
      {error ? (
        <p role="alert">{text('无法读取历史记录。', 'Could not load history.')}</p>
      ) : history === null ? (
        <p>{text('加载中…', 'Loading…')}</p>
      ) : (
        <>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(170px, 1fr))',
              gap: 12,
            }}
          >
            {stats.map(([label, value]) => (
              <Card key={String(label)}>
                <div style={{ color: 'var(--ol-ink-3)', fontSize: 13 }}>{label}</div>
                <div style={{ fontSize: 26, fontWeight: 600, marginTop: 8 }}>{value}</div>
              </Card>
            ))}
          </div>
          <Card style={{ marginTop: 14 }}>
            <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
              {text('每日记录', 'Daily records')}
            </h2>
            <div style={{ display: 'flex', alignItems: 'end', gap: 4, height: 120 }}>
              {daily.map(({ date, count }) => (
                <div
                  key={date.toISOString()}
                  title={`${date.toLocaleDateString()}: ${count}`}
                  style={{
                    flex: 1,
                    minWidth: 2,
                    height: `${Math.max(2, (count / maxDaily) * 100)}%`,
                    background: count ? 'var(--ol-blue)' : 'var(--ol-surface-2)',
                    borderRadius: 3,
                  }}
                />
              ))}
            </div>
          </Card>
          <Card style={{ marginTop: 14 }}>
            <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
              {text('服务与用途', 'Services and purposes')}
            </h2>
            {grouped.size === 0 ? (
              <p>{text('没有可归属服务的记录。', 'No attributable records.')}</p>
            ) : (
              [...grouped.entries()]
                .sort((a, b) => b[1] - a[1])
                .map(([name, count]) => (
                  <div
                    key={name}
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      gap: 12,
                      padding: '10px 0',
                      borderTop: '1px solid var(--ol-line)',
                    }}
                  >
                    <span>{name}</span>
                    <span>{count}</span>
                  </div>
                ))
            )}
          </Card>
          <Card style={{ marginTop: 14 }}>
            <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
              {text('已报告的模型用量', 'Reported model usage')}
            </h2>
            <p style={{ color: 'var(--ol-ink-3)', fontSize: 12 }}>
              {text(
                '缓存和推理字段由各服务商报告，可能已包含在输入或输出中，不能直接相加。',
                'Provider-reported cache and reasoning fields may already be included in input or output; do not add them directly.',
              )}
            </p>
            {byPurpose.map(({ purpose, input, output }) => (
              <div
                key={purpose}
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  gap: 12,
                  padding: '8px 0',
                }}
              >
                <strong>{purposeName(purpose)}</strong>
                <span>
                  {text('已报告输入', 'Reported input')}: {input?.toLocaleString() ?? '—'} ·{' '}
                  {text('已报告输出', 'Reported output')}: {output?.toLocaleString() ?? '—'}
                </span>
              </div>
            ))}
            {reported.length === 0 ? (
              <p>
                {text(
                  '该时段没有已保存的服务商 token 报告。',
                  'No provider token reports were saved for this period.',
                )}
              </p>
            ) : (
              [...reported]
                .sort((a, b) => b.occurredAtMs - a.occurredAtMs)
                .slice(0, 30)
                .map((record) => (
                  <div
                    key={record.id}
                    style={{
                      display: 'grid',
                      gridTemplateColumns: 'repeat(auto-fit, minmax(140px, 1fr))',
                      gap: 8,
                      padding: '10px 0',
                      borderTop: '1px solid var(--ol-line)',
                    }}
                  >
                    <time>{new Date(record.occurredAtMs).toLocaleString()}</time>
                    <span>{purposeName(record.purpose)}</span>
                    <span>
                      {[
                        ...new Set([record.provider, record.channel, record.model].filter(Boolean)),
                      ].join(' · ') || '—'}
                    </span>
                    <span>
                      {text('输入', 'Input')}: {safeTokens(record.inputTokens)} ·{' '}
                      {text('输出', 'Output')}: {safeTokens(record.outputTokens)}
                      <br />
                      {text('缓存读取', 'Cache read')}: {safeTokens(record.cachedInputTokens)} ·{' '}
                      {text('缓存写入', 'Cache write')}: {safeTokens(record.cacheWriteInputTokens)}
                      <br />
                      {text('推理', 'Reasoning')}: {safeTokens(record.reasoningTokens)}
                    </span>
                  </div>
                ))
            )}
          </Card>
          <Card style={{ marginTop: 14 }}>
            <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
              {text('最近记录', 'Recent records')}
            </h2>
            {rows.slice(0, 30).map((row) => (
              <div
                key={row.id}
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  gap: 12,
                  padding: '10px 0',
                  borderTop: '1px solid var(--ol-line)',
                }}
              >
                <time dateTime={row.createdAt}>{new Date(row.createdAt).toLocaleString()}</time>
                <span>{[row.asrProvider, row.llmProvider].filter(Boolean).join(' · ') || '—'}</span>
              </div>
            ))}
          </Card>
        </>
      )}
    </>
  );
}
