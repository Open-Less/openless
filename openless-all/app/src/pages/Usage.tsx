import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listHistory } from '../lib/ipc/history';
import type { DictationSession } from '../lib/types';
import { Card, PageHeader } from './_atoms';

export function Usage() {
  const { i18n } = useTranslation();
  const zh = i18n.resolvedLanguage?.startsWith('zh') ?? false;
  const text = (cn: string, en: string) => (zh ? cn : en);
  const [history, setHistory] = useState<DictationSession[] | null>(null);
  const [error, setError] = useState(false);
  const [days, setDays] = useState(30);
  useEffect(() => {
    let live = true;
    void listHistory().then(
      (rows) => {
        if (live) setHistory(rows);
      },
      () => {
        if (live) setError(true);
      },
    );
    return () => {
      live = false;
    };
  }, []);
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
  const seconds = rows.reduce((total, row) => total + (row.durationMs ?? 0), 0) / 1000;
  const stats = [
    [text('语音识别记录', 'ASR records'), asr.length],
    [text('文字处理记录', 'Text processing records'), llm.length],
    [text('多模态记录', 'Multimodal records'), omni.length],
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
      />
      <p style={{ color: 'var(--ol-ink-3)', fontSize: 13, lineHeight: 1.6 }}>
        {text(
          '当前仅统计本机保留的听写历史（最多 200 条），不含其他工具调用。历史未记录的 token、费用和 TTS 用量暂不可用。',
          'These figures cover retained dictation history only (up to 200 entries), not other tools. Tokens, costs, and TTS usage were not recorded and are unavailable.',
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
