import type { buildUsageDashboard, UsageStageKind } from './model';
import { useUsageLabels } from './labels';
import { Icon } from '../../components/Icon';
import { Pill, type PillTone } from '../../ui/controls/Pill';
import { Card } from '../../ui/layout/Card';
import { formatUsageNumber } from '../../lib/serviceUsageMetrics';

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

function repeatsModelName(label: string, model: string | null) {
  if (!model) return false;
  const normalize = (value: string) => value.toLowerCase().replace(/[^a-z0-9]+/g, '-');
  const prefix = normalize(label);
  const name = normalize(model);
  return name === prefix || (prefix.startsWith('deepseek') && name.startsWith(prefix + '-'));
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
                const metaParts = [...new Set([serviceName(group.service), group.channel])]
                  .filter((label) => label && !repeatsModelName(label, group.model));
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
                          {!repeatsModelName(item.provider, item.model) && (
                            <span className="ol-usage-service-provider">{item.provider}</span>
                          )}
                          {item.model && (
                            <>
                              {!repeatsModelName(item.provider, item.model) && (
                                <span className="ol-usage-sep" aria-hidden="true">·</span>
                              )}
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

export function UsagePurpose({ dashboard }: { dashboard: Dashboard }) {
  const { text, purposeName } = useUsageLabels();
  const { cachePercent, modelPurposes, ledgerUnavailable } = dashboard;

  return (
    <section aria-label={text('已报告的模型用量', 'Reported model usage')}>
      {ledgerUnavailable && (
        <p className="ol-usage-empty">
          {text('本地账本暂不可用。', 'The local ledger is unavailable.')}
        </p>
      )}
      <div className="ol-grid-12 ol-grid-stats ol-usage-metrics ol-usage-model-purposes">
        {modelPurposes.map(({ purpose, totalRecords, input, output, reads, writes, hitRate }) => (
          <Card key={purpose} className="ol-usage-metric-card" padding={14}>
            <h2 className="ol-usage-card-title">{purposeName(purpose)}</h2>
            <dl>
              {[
                { label: text('输入 Token', 'Input tokens'), ...input },
                { label: text('输出 Token', 'Output tokens'), ...output },
                { label: text('缓存读取', 'Cache reads'), ...reads },
                { label: text('缓存写入', 'Cache writes'), ...writes },
                { label: text('缓存命中率', 'Cache hit rate'), ...hitRate },
              ].map(({ label, value, count }, index) => (
                <div key={label}>
                  <dt>{label}</dt>
                  <dd data-empty={value === null || undefined}>
                    {index === 4 ? cachePercent(value) : formatUsageNumber(value)}
                    {count > 0 && count < totalRecords && (
                      <span
                        className="ol-usage-metric-badge"
                        title={text('此项报告覆盖率', 'Reports covering this metric')}
                      >
                        {count}/{totalRecords}
                      </span>
                    )}
                  </dd>
                </div>
              ))}
            </dl>
          </Card>
        ))}
      </div>
    </section>
  );
}
