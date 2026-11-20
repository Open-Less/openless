import type { buildUsageDashboard, UsageStageKind } from './model';
import { useUsageLabels } from './labels';
import { Icon } from '../../components/Icon';
import { Pill, type PillTone } from '../../ui/controls/Pill';
import { Card } from '../../ui/layout/Card';

type Dashboard = ReturnType<typeof buildUsageDashboard>;

function stageTone(kind: UsageStageKind): PillTone {
  if (kind === 'postprocess' || kind === 'multimodal') return 'blue';
  if (kind === 'translation') return 'ok';
  return 'outline';
}

export function UsageOverview({ dashboard }: { dashboard: Dashboard }) {
  const { text } = useUsageLabels();
  const { days, daily, totalDaily, maxDaily, metricItems, historyUnavailable } = dashboard;
  const dense = daily.length > 7;
  const lastIndex = daily.length - 1;
  const midIndex = Math.floor(lastIndex / 2);
  const dailyAverage = days > 0 ? totalDaily / days : 0;
  const avgDisplay =
    dailyAverage >= 10 ? Math.round(dailyAverage) : Number(dailyAverage.toFixed(1));
  const formatShortDate = (date: Date) =>
    date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });

  return (
    <>
      <div className="ol-grid-12 ol-grid-stats ol-usage-metrics">
        {metricItems.map((item) => (
          <Card key={item.id} className="ol-usage-metric-card" padding={14}>
            <div className="ol-usage-metric-head">
              <span className="ol-usage-metric-label">
                <Icon name={item.icon} size={13} />
                <span>{item.label}</span>
              </span>
              {item.badge && (
                <span className="ol-usage-metric-badge" title={item.badgeTitle}>
                  {item.badge}
                </span>
              )}
            </div>
            <div
              className="ol-usage-metric-value"
              data-numeric
              data-empty={item.empty || undefined}
              title={item.exact}
            >
              {item.value}
            </div>
          </Card>
        ))}
      </div>
      {!historyUnavailable && (
        <Card className="ol-usage-chart-card" padding={18}>
          <div className="ol-usage-chart-header">
            <div className="ol-usage-chart-title-group">
              <h2 className="ol-usage-card-title">{text('每日记录', 'Daily records')}</h2>
              <span className="ol-usage-chart-summary">
                {text(
                  `共 ${totalDaily} 条 · 日均 ${avgDisplay}`,
                  `${totalDaily} total · ${avgDisplay}/day`,
                )}
              </span>
            </div>
            {totalDaily > 0 && (
              <span className="ol-usage-count-chip">
                {text(`峰值 ${maxDaily}/天`, `Peak ${maxDaily}/day`)}
              </span>
            )}
          </div>
          <div className="ol-usage-chart" data-dense={dense || undefined}>
            {daily.map(({ date, count }, index) => {
              const isToday = index === lastIndex;
              const pct = count === 0 ? 0 : Math.max(6, (count / maxDaily) * 100);
              return (
                <div
                  key={date.toISOString()}
                  className="ol-usage-chart-col"
                  title={`${date.toLocaleDateString()}: ${count}`}
                >
                  {!dense && (
                    <span
                      className="ol-usage-chart-val"
                      data-today={isToday || undefined}
                      data-zero={count === 0 || undefined}
                    >
                      {count}
                    </span>
                  )}
                  <div
                    className="ol-usage-chart-bar"
                    data-today={isToday || undefined}
                    data-active={count > 0 || undefined}
                    style={{ height: count > 0 ? `${pct}%` : '3px' }}
                  />
                </div>
              );
            })}
          </div>
          <div className="ol-usage-chart-dates">
            {dense ? (
              <>
                <span>{formatShortDate(daily[0].date)}</span>
                <span>{formatShortDate(daily[midIndex].date)}</span>
                <span>{formatShortDate(daily[lastIndex].date)}</span>
              </>
            ) : (
              daily.map(({ date }) => <span key={date.toISOString()}>{formatShortDate(date)}</span>)
            )}
          </div>
        </Card>
      )}
    </>
  );
}

export function UsageServices({ dashboard }: { dashboard: Dashboard }) {
  const { text, protocolName, serviceName } = useUsageLabels();
  const {
    cacheGroups,
    cachePercent,
    serviceItems,
    totalServiceCalls,
    historyUnavailable,
    ledgerUnavailable,
  } = dashboard;

  return (
    <div className="ol-grid-12 ol-usage-services-grid">
      <Card className="ol-usage-panel-card" padding={14}>
        <div className="ol-usage-card-header">
          <h2 className="ol-usage-card-title">{text('模型缓存报告', 'Model cache reports')}</h2>
          {!ledgerUnavailable && cacheGroups.length > 0 && (
            <span className="ol-usage-count-chip">
              {text(`${cacheGroups.length} 个模型`, `${cacheGroups.length} models`)}
            </span>
          )}
        </div>
        <div className="ol-usage-inset-frame">
          <div
            className="ol-usage-panel-scroll ol-thinscroll"
            role="region"
            aria-label={text('模型缓存报告列表', 'Model cache report list')}
            tabIndex={0}
          >
            {ledgerUnavailable ? (
              <div className="ol-usage-empty">
                {text('本地账本暂不可用。', 'The local ledger is unavailable.')}
              </div>
            ) : cacheGroups.length === 0 ? (
              <div className="ol-usage-empty">
                {text('尚无模型缓存报告。', 'No model cache reports yet.')}
              </div>
            ) : (
              cacheGroups.map((group) => {
                const metaParts = [serviceName(group.service), group.channel].filter(Boolean);
                const ratePct =
                  group.rate === null ? 0 : Math.min(100, Math.max(0, group.rate * 100));
                return (
                  <div key={group.key} className="ol-usage-cache-row">
                    <div className="ol-usage-cache-main">
                      <div className="ol-usage-model-name">
                        {group.model ?? text('未知模型', 'Unknown model')}
                      </div>
                      <div className="ol-usage-cache-meta">
                        <span>{metaParts.join(' · ')}</span>
                        {group.protocol && (
                          <Pill size="sm" tone="outline">
                            {protocolName(group.protocol)}
                          </Pill>
                        )}
                      </div>
                    </div>
                    <div className="ol-usage-cache-stat">
                      <div
                        className="ol-usage-cache-rate"
                        data-empty={group.rate === null || undefined}
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
                      </div>
                      <div className="ol-usage-meter" aria-hidden="true">
                        <div
                          className="ol-usage-meter-fill"
                          data-active={group.rate !== null && group.rate > 0 ? 'true' : undefined}
                          style={{ width: `${ratePct}%` }}
                        />
                      </div>
                      <div className="ol-usage-cache-sub">
                        {text(
                          `${group.comparableRecords}/${group.totalRecords} 条可比较报告`,
                          `${group.comparableRecords}/${group.totalRecords} comparable reports`,
                        )}
                      </div>
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </Card>

      <Card className="ol-usage-panel-card" padding={14}>
        <div className="ol-usage-card-header">
          <h2 className="ol-usage-card-title">{text('服务与用途', 'Services and purposes')}</h2>
          {!historyUnavailable && totalServiceCalls > 0 && (
            <span className="ol-usage-count-chip">
              {text(`${totalServiceCalls} 次调用`, `${totalServiceCalls} calls`)}
            </span>
          )}
        </div>
        <div className="ol-usage-inset-frame">
          <div
            className="ol-usage-panel-scroll ol-thinscroll"
            role="region"
            aria-label={text('服务与用途列表', 'Services and purposes list')}
            tabIndex={0}
          >
            {historyUnavailable ? (
              <div className="ol-usage-empty">
                {text('历史记录暂不可用。', 'Retained history is unavailable.')}
              </div>
            ) : serviceItems.length === 0 ? (
              <div className="ol-usage-empty">
                {text('没有可归属服务的记录。', 'No attributable records.')}
              </div>
            ) : (
              serviceItems.map((item) => {
                const barPct = Math.max(4, item.share * 100);
                const sharePct = Math.round(item.share * 100);
                return (
                  <div key={item.key} className="ol-usage-service-row">
                    <div className="ol-usage-service-top">
                      <div className="ol-usage-service-id">
                        <Pill size="sm" tone={stageTone(item.stageKind)}>
                          {item.stage}
                        </Pill>
                        <span className="ol-usage-service-target">
                          <span className="ol-usage-service-provider">{item.provider}</span>
                          {item.model && (
                            <>
                              <span className="ol-usage-sep" aria-hidden="true">
                                ·
                              </span>
                              <span className="ol-usage-service-model">{item.model}</span>
                            </>
                          )}
                        </span>
                      </div>
                      <div className="ol-usage-service-count">
                        <strong>{item.count}</strong>
                        <span>{sharePct}%</span>
                      </div>
                    </div>
                    <div className="ol-usage-meter" aria-hidden="true">
                      <div
                        className="ol-usage-meter-fill"
                        data-active="true"
                        style={{ width: `${barPct}%` }}
                      />
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </Card>
    </div>
  );
}

export function UsageHistory({ dashboard }: { dashboard: Dashboard }) {
  const { text, purposeName, purposeTone, safeTokens } = useUsageLabels();
  const { byPurpose, reported, rows, historyUnavailable, ledgerUnavailable } = dashboard;
  const sortedReported = [...reported].sort((a, b) => b.occurredAtMs - a.occurredAtMs).slice(0, 30);

  return (
    <div className="ol-usage-history-grid">
      <Card className="ol-usage-record-card ol-usage-reported-card" padding={14}>
        <div className="ol-usage-card-header ol-usage-card-header--stacked">
          <div className="ol-usage-card-header-row">
            <h2 className="ol-usage-card-title">
              {text('已报告的模型用量', 'Reported model usage')}
            </h2>
            {!ledgerUnavailable && (
              <span className="ol-usage-count-chip">
                {text(`${reported.length} 条`, `${reported.length}`)}
              </span>
            )}
          </div>
          <p className="ol-usage-card-note">
            <Icon name="info" size={12} />
            <span>
              {text(
                '缓存和推理字段由各服务商报告，可能已包含在输入或输出中，不能直接相加。',
                'Provider-reported cache and reasoning fields may already be included in input or output; do not add them directly.',
              )}
            </span>
          </p>
        </div>
        <div className="ol-usage-inset-frame">
          <div
            className="ol-usage-record-scroll ol-thinscroll"
            tabIndex={0}
            role="region"
            aria-label={text('模型用量列表', 'Model usage list')}
          >
            {byPurpose.length > 0 && (
              <div className="ol-usage-purpose-grid">
                {byPurpose.map(({ purpose, input, output }) => (
                  <div key={purpose} className="ol-usage-purpose-tile">
                    <div className="ol-usage-purpose-head">
                      <Pill size="sm" tone={purposeTone(purpose)}>
                        {purposeName(purpose)}
                      </Pill>
                    </div>
                    <div className="ol-usage-purpose-metrics">
                      <div className="ol-usage-purpose-metric">
                        <span className="ol-usage-metric-key">
                          {text('已报告输入', 'Reported input')}
                        </span>
                        {safeTokens(input)}
                      </div>
                      <div className="ol-usage-purpose-metric">
                        <span className="ol-usage-metric-key">
                          {text('已报告输出', 'Reported output')}
                        </span>
                        {safeTokens(output)}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
            {ledgerUnavailable ? (
              <div className="ol-usage-empty">
                {text('本地账本暂不可用。', 'The local ledger is unavailable.')}
              </div>
            ) : sortedReported.length === 0 ? (
              <div className="ol-usage-empty">
                {text(
                  '该时段没有已保存的服务商 token 报告。',
                  'No provider token reports were saved for this period.',
                )}
              </div>
            ) : (
              <div className="ol-usage-report-list">
                {sortedReported.map((record) => {
                  const targetParts = [
                    ...new Set([record.provider, record.channel, record.model].filter(Boolean)),
                  ];
                  return (
                    <div key={record.id} className="ol-usage-report-item">
                      <div className="ol-usage-report-head">
                        <div className="ol-usage-report-identity">
                          <Pill size="sm" tone={purposeTone(record.purpose)}>
                            {purposeName(record.purpose)}
                          </Pill>
                          <span className="ol-usage-report-target">
                            {targetParts.join(' · ') || '—'}
                          </span>
                        </div>
                        <time className="ol-usage-time">
                          {new Date(record.occurredAtMs).toLocaleString()}
                        </time>
                      </div>
                      <div className="ol-usage-token-grid">
                        <div className="ol-usage-token-cell">
                          <span className="ol-usage-token-label">{text('输入', 'Input')}</span>
                          {safeTokens(record.inputTokens)}
                        </div>
                        <div className="ol-usage-token-cell">
                          <span className="ol-usage-token-label">{text('输出', 'Output')}</span>
                          {safeTokens(record.outputTokens)}
                        </div>
                        <div className="ol-usage-token-cell">
                          <span className="ol-usage-token-label">
                            {text('缓存读取', 'Cache read')}
                          </span>
                          {safeTokens(record.cachedInputTokens)}
                        </div>
                        <div className="ol-usage-token-cell">
                          <span className="ol-usage-token-label">
                            {text('缓存写入', 'Cache write')}
                          </span>
                          {safeTokens(record.cacheWriteInputTokens)}
                        </div>
                        <div className="ol-usage-token-cell">
                          <span className="ol-usage-token-label">{text('推理', 'Reasoning')}</span>
                          {safeTokens(record.reasoningTokens)}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </Card>

      <Card className="ol-usage-record-card ol-usage-recent-card" padding={14}>
        <div className="ol-usage-card-header">
          <h2 className="ol-usage-card-title">{text('最近记录', 'Recent records')}</h2>
          {!historyUnavailable && rows.length > 0 && (
            <span className="ol-usage-count-chip">
              {text(`${rows.length} 条`, `${rows.length}`)}
            </span>
          )}
        </div>
        <div className="ol-usage-inset-frame">
          <div
            className="ol-usage-record-scroll ol-thinscroll"
            tabIndex={0}
            role="region"
            aria-label={text('最近记录列表', 'Recent records list')}
          >
            {historyUnavailable ? (
              <div className="ol-usage-empty">
                {text('历史记录暂不可用。', 'Retained history is unavailable.')}
              </div>
            ) : rows.length === 0 ? (
              <div className="ol-usage-empty">
                {text('该时段暂无记录。', 'No records in this period.')}
              </div>
            ) : (
              rows.map((row) => {
                const providers = [row.asrProvider, row.llmProvider].filter(Boolean);
                return (
                  <div key={row.id} className="ol-usage-recent-row">
                    <time dateTime={row.createdAt} className="ol-usage-time">
                      {new Date(row.createdAt).toLocaleString()}
                    </time>
                    <div className="ol-usage-provider-tags">
                      {providers.length > 0 ? (
                        providers.map((provider, index) => (
                          <span key={`${provider}-${index}`} className="ol-usage-provider-tag">
                            {provider}
                          </span>
                        ))
                      ) : (
                        <span className="ol-usage-token-value" data-empty="true">
                          —
                        </span>
                      )}
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </Card>
    </div>
  );
}
