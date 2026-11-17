import type { buildUsageDashboard } from './model';
import { useUsageLabels } from './labels';
import { Card } from '../../ui/layout/Card';

type Dashboard = ReturnType<typeof buildUsageDashboard>;

export function UsageOverview({ dashboard }: { dashboard: Dashboard }) {
  const { text } = useUsageLabels();
  const { daily, maxDaily, stats, cacheSummary, historyUnavailable, ledgerUnavailable } = dashboard;
  return (
    <>
      <div className="ol-grid-12 ol-grid-stats">
        {stats.map(([label, value, exact]) => (
          <Card key={String(label)}>
            <div style={{ color: 'var(--ol-ink-3)', fontSize: 13 }}>
              {label}
              {!ledgerUnavailable && label === text('缓存命中率', 'Cache hit rate') && (
                <span
                  style={{ marginLeft: 6, fontSize: 11 }}
                  title={text('可比较报告 / 已保存报告', 'Comparable reports / saved reports')}
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
      {!historyUnavailable && (
        <Card style={{ marginTop: 14 }}>
          <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>{text('每日记录', 'Daily records')}</h2>
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
      )}
    </>
  );
}

export function UsageServices({ dashboard }: { dashboard: Dashboard }) {
  const { text, protocolName, serviceName } = useUsageLabels();
  const { cacheGroups, cachePercent, grouped, historyUnavailable, ledgerUnavailable } = dashboard;
  return (
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
          {ledgerUnavailable ? (
            <p>{text('本地账本暂不可用。', 'The local ledger is unavailable.')}</p>
          ) : cacheGroups.length === 0 ? (
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
                    {[serviceName(group.service), group.channel].filter(Boolean).join(' · ')}
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
        {historyUnavailable ? (
          <p>{text('历史记录暂不可用。', 'Retained history is unavailable.')}</p>
        ) : grouped.size === 0 ? (
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
  );
}

export function UsageHistory({ dashboard }: { dashboard: Dashboard }) {
  const { text, purposeName, safeTokens } = useUsageLabels();
  const { byPurpose, reported, rows, historyUnavailable, ledgerUnavailable } = dashboard;
  return (
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
          {ledgerUnavailable ? (
            <p>{text('本地账本暂不可用。', 'The local ledger is unavailable.')}</p>
          ) : reported.length === 0 ? (
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
                  className="ol-grid-12 ol-grid-quarters"
                  style={{
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
        </div>
      </Card>
      <Card className="ol-usage-record-card">
        <h2 style={{ fontSize: 16, margin: '0 0 14px' }}>{text('最近记录', 'Recent records')}</h2>
        <div
          className="ol-usage-record-scroll ol-thinscroll"
          tabIndex={0}
          role="region"
          aria-label={text('最近记录列表', 'Recent records list')}
        >
          {historyUnavailable && (
            <p>{text('历史记录暂不可用。', 'Retained history is unavailable.')}</p>
          )}
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
              <time dateTime={row.createdAt}>{new Date(row.createdAt).toLocaleString()}</time>
              <span>{[row.asrProvider, row.llmProvider].filter(Boolean).join(' · ') || '—'}</span>
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}
