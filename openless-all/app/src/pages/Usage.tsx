import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listHistory } from '../lib/ipc/history';
import {
  clearServiceUsage,
  getServiceUsage,
  type ServiceUsageSnapshot,
} from '../lib/ipc/service-usage';
import type { DictationSession } from '../lib/types';
import {
  summarizeReportedUsage,
  formatUsageNumber,
  summarizeCacheUsage,
  summarizeCacheUsageByModel,
} from '../lib/serviceUsageMetrics';
import { Segmented } from '../components/ui/Segmented';
import './usage.css';
import { Btn, Card, PageHeader } from './_atoms';

export function Usage() {
  const { t, i18n } = useTranslation();
  const zh = i18n.resolvedLanguage?.startsWith('zh') ?? false;
  const text = (cn: string, en: string) => (zh ? cn : en);
  const serviceName = (service: string | null) => {
    if (!service) return text('未知服务', 'Unknown service');
    const labels: Record<string, string> = {
      anthropic: 'Claude · Anthropic',
      dashscope: text('阿里云百炼', 'Alibaba Cloud Bailian'),
      openrouter: 'OpenRouter',
      zhipu: text('智谱', 'Zhipu'),
      groq: 'Groq',
      zenmux: 'ZenMux',
      minimax: 'MiniMax',
    };
    return (
      labels[service] ??
      t(`settings.providers.presets.${service === 'api-route' ? 'apiRoute' : service}`, {
        defaultValue: service,
      })
    );
  };
  const protocolName = (protocol: string | null) =>
    ({ chat_completions: 'Chat Completions', responses: 'Responses', messages: 'Messages' })[
      protocol ?? ''
    ] ??
    protocol ??
    text('未知协议', 'Unknown protocol');
  const [history, setHistory] = useState<DictationSession[] | null>(null);
  const [error, setError] = useState(false);
  const [days, setDays] = useState(30);
  const [view, setView] = useState<'overview' | 'services' | 'history'>('overview');
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
  const safeTokens = (value: number | null) => (
    <span
      title={
        value !== null && Number.isSafeInteger(value) && value >= 0
          ? value.toLocaleString()
          : undefined
      }
    >
      {formatUsageNumber(value)}
    </span>
  );
  const cacheSummary = summarizeCacheUsage(reported);
  const cacheRate = cacheSummary.rate;
  const cacheGroups = summarizeCacheUsageByModel(reported);
  const cachePercent = (rate: number | null) =>
    rate === null ? '—' : `${(rate * 100).toFixed(1)}%`;
  const seconds = rows.reduce((total, row) => total + (row.durationMs ?? 0), 0) / 1000;
  const stats: Array<[string, string, string?]> = [
    [text('语音识别记录', 'ASR records'), formatUsageNumber(asr.length), String(asr.length)],
    [
      text('文字处理记录', 'Text processing records'),
      formatUsageNumber(llm.length),
      String(llm.length),
    ],
    [text('多模态记录', 'Multimodal records'), formatUsageNumber(omni.length), String(omni.length)],
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
  const clearControl = view === 'history' && usage && (
    <Btn
      style={{
        minHeight: 36,
        lineHeight: 1.4,
        flexShrink: 0,
        whiteSpace: 'nowrap',
      }}
      disabled={clearing}
      onClick={async () => {
        clearingRef.current = true;
        request.current += 1;
        setClearing(true);
        setClearError(false);
        try {
          await clearServiceUsage();
          if (mounted.current) setUsage({ records: [], droppedOrFailed: usage.droppedOrFailed });
        } catch {
          if (mounted.current) setClearError(true);
        } finally {
          clearingRef.current = false;
          if (mounted.current) setClearing(false);
        }
      }}
    >
      {text('清除已记录 Token', 'Clear recorded tokens')}
    </Btn>
  );
  return (
    <div className="ol-usage-page" style={{ width: '100%', minWidth: 0, flexShrink: 0 }}>
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
      <nav
        aria-label={text('用量页面', 'Usage pages')}
        style={{
          display: 'flex',
          gap: 8,
          margin: '0 0 20px',
          borderBottom: '1px solid var(--ol-line)',
        }}
      >
        {(
          [
            ['overview', text('概览', 'Overview')],
            ['services', text('服务与用途', 'Services and purposes')],
            ['history', text('记录', 'History')],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            aria-current={view === id ? 'page' : undefined}
            onClick={() => setView(id)}
            style={{
              border: 0,
              borderBottom: view === id ? '2px solid var(--ol-blue)' : '2px solid transparent',
              background: 'transparent',
              color: view === id ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
              padding: '10px 12px',
              font: 'inherit',
              fontWeight: view === id ? 600 : 400,
              cursor: 'pointer',
            }}
          >
            {label}
          </button>
        ))}
      </nav>
      <div className="ol-usage-toolbar">
        <Segmented
          label={text('统计时段', 'Usage period')}
          options={[
            { value: '7', label: text('近 7 天', 'Last 7 days') },
            { value: '30', label: text('近 30 天', 'Last 30 days') },
          ]}
          value={String(days)}
          onChange={(value) => setDays(Number(value))}
        />
        {clearControl}
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
      {error ? (
        <p role="alert">{text('无法读取历史记录。', 'Could not load history.')}</p>
      ) : history === null ? (
        <p>{text('加载中…', 'Loading…')}</p>
      ) : (
        <>
          {view === 'overview' && (
            <>
              <div
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
                  gap: 12,
                }}
              >
                {stats.map(([label, value, exact]) => (
                  <Card key={String(label)}>
                    <div style={{ color: 'var(--ol-ink-3)', fontSize: 13 }}>
                      {label}
                      {label === text('缓存命中率', 'Cache hit rate') && (
                        <span
                          style={{ marginLeft: 6, fontSize: 11 }}
                          title={text(
                            '可比较报告 / 已保存报告',
                            'Comparable reports / saved reports',
                          )}
                        >
                          {cacheSummary.comparableRecords}/{cacheSummary.totalRecords}
                        </span>
                      )}
                    </div>
                    <div title={exact} style={{ fontSize: 26, fontWeight: 600, marginTop: 8 }}>
                      {value}
                    </div>
                  </Card>
                ))}
              </div>
              <Card style={{ marginTop: 14 }}>
                <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
                  {text('每日记录', 'Daily records')}
                </h2>
                <div className="ol-usage-chart">
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
                <div className="ol-usage-chart-dates">
                  <span>
                    {daily[0].date.toLocaleDateString(undefined, {
                      month: 'short',
                      day: 'numeric',
                    })}
                  </span>
                  <span>
                    {daily[daily.length - 1].date.toLocaleDateString(undefined, {
                      month: 'short',
                      day: 'numeric',
                    })}
                  </span>
                </div>
              </Card>
            </>
          )}
          {view === 'services' && (
            <>
              <Card style={{ marginTop: 14 }}>
                <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
                  {text('模型缓存报告', 'Model cache reports')}
                </h2>
                <div
                  className="ol-thinscroll"
                  role="region"
                  aria-label={text('模型缓存报告列表', 'Model cache report list')}
                  tabIndex={0}
                  style={{
                    maxHeight: 'min(440px, 55dvh)',
                    overflowY: 'auto',
                    overscrollBehavior: 'contain',
                  }}
                >
                  {cacheGroups.length === 0 ? (
                    <p>{text('尚无模型缓存报告。', 'No model cache reports yet.')}</p>
                  ) : (
                    cacheGroups.map((group) => (
                      <div
                        key={group.key}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 16,
                          padding: '12px 0',
                          borderTop: '1px solid var(--ol-line)',
                        }}
                      >
                        <div style={{ minWidth: 0, overflowWrap: 'anywhere' }}>
                          <strong>{group.model ?? text('未知模型', 'Unknown model')}</strong>
                          <div
                            title={protocolName(group.protocol)}
                            style={{ color: 'var(--ol-ink-3)', fontSize: 12, marginTop: 4 }}
                          >
                            {[serviceName(group.service), group.channel]
                              .filter(Boolean)
                              .join(' · ')}
                          </div>
                        </div>
                        <div style={{ textAlign: 'right', flexShrink: 0 }}>
                          <strong
                            title={
                              group.rate === null
                                ? text(
                                    '缺少可比较的缓存数据（含旧记录）',
                                    'Comparable cache data unavailable, including legacy records',
                                  )
                                : text(
                                    '输入 Token 缓存占比，非请求命中比例',
                                    'Cached input token share, not the fraction of requests with a hit',
                                  )
                            }
                          >
                            {cachePercent(group.rate)}
                          </strong>
                          <div style={{ color: 'var(--ol-ink-3)', fontSize: 12, marginTop: 4 }}>
                            {text(
                              `${group.comparableRecords}/${group.totalRecords} 条可比较报告`,
                              `${group.comparableRecords}/${group.totalRecords} comparable reports`,
                            )}
                          </div>
                        </div>
                      </div>
                    ))
                  )}
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
            </>
          )}
          {view === 'history' && (
            <div className="ol-usage-history-grid">
              <Card className="ol-usage-record-card">
                <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
                  {text('已报告的模型用量', 'Reported model usage')}
                </h2>
                <p style={{ color: 'var(--ol-ink-3)', fontSize: 12 }}>
                  {text(
                    '缓存和推理字段由各服务商报告，可能已包含在输入或输出中，不能直接相加。',
                    'Provider-reported cache and reasoning fields may already be included in input or output; do not add them directly.',
                  )}
                </p>
                <div
                  className="ol-usage-record-scroll ol-thinscroll"
                  tabIndex={0}
                  role="region"
                  aria-label={text('模型用量列表', 'Model usage list')}
                >
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
                        {text('已报告输入', 'Reported input')}: {safeTokens(input)} ·{' '}
                        {text('已报告输出', 'Reported output')}: {safeTokens(output)}
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
                              ...new Set(
                                [record.provider, record.channel, record.model].filter(Boolean),
                              ),
                            ].join(' · ') || '—'}
                          </span>
                          <span>
                            {text('输入', 'Input')}: {safeTokens(record.inputTokens)} ·{' '}
                            {text('输出', 'Output')}: {safeTokens(record.outputTokens)}
                            <br />
                            {text('缓存读取', 'Cache read')}: {safeTokens(record.cachedInputTokens)}{' '}
                            · {text('缓存写入', 'Cache write')}:{' '}
                            {safeTokens(record.cacheWriteInputTokens)}
                            <br />
                            {text('推理', 'Reasoning')}: {safeTokens(record.reasoningTokens)}
                          </span>
                        </div>
                      ))
                  )}
                </div>
              </Card>
              <Card className="ol-usage-record-card">
                <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>
                  {text('最近记录', 'Recent records')}
                </h2>
                <div
                  className="ol-usage-record-scroll ol-thinscroll"
                  tabIndex={0}
                  role="region"
                  aria-label={text('最近记录列表', 'Recent records list')}
                >
                  {rows.map((row) => (
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
                      <time dateTime={row.createdAt}>
                        {new Date(row.createdAt).toLocaleString()}
                      </time>
                      <span>
                        {[row.asrProvider, row.llmProvider].filter(Boolean).join(' · ') || '—'}
                      </span>
                    </div>
                  ))}
                </div>
              </Card>
            </div>
          )}
        </>
      )}
    </div>
  );
}
