import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { clearServiceUsage, supportsServiceUsage } from '../../api/usage';
import { Icon } from '../../components/Icon';
import { useHistoryRecords } from '../../data/history';
import { refreshServiceUsage, useServiceUsage } from '../../data/usage';
import { Segmented } from '../../ui/controls/Segmented';
import { Button as Btn } from '../../ui/controls/Button';
import { PageHeader } from '../../ui/layout/PageHeader';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { buildUsageDashboard } from './model';
import { useUsageLabels } from './labels';
import { UsageOverview, UsageServices, UsagePurpose } from './UsageViews';
import './usage.css';

export function Usage() {
  const { t } = useTranslation();
  const { text } = useUsageLabels();
  const { confirm, notify } = useInteraction('usage');
  const history = useHistoryRecords();
  const serviceUsage = useServiceUsage();
  const [days, setDays] = useState(30);
  const [view, setView] = useState<'overview' | 'purpose' | 'services'>('overview');
  const [clearing, setClearing] = useState(false);
  const clearPending = useRef(false);
  const rows = useMemo(() => {
    if (!history.data) return null;
    const cutoff = Date.now() - days * 86_400_000;
    return history.data.filter(
      (row) =>
        (!row.source || row.source === 'voice') && new Date(row.createdAt).getTime() >= cutoff,
    );
  }, [history.data, days]);
  const dashboard = buildUsageDashboard(rows, serviceUsage.data?.records ?? null, days, text);

  const clear = async () => {
    if (clearPending.current) return;
    clearPending.current = true;
    try {
      await confirm({
        title: t('usage.clear'),
        description: t('usage.clearScope'),
        confirmLabel: t('usage.clear'),
        tone: 'danger',
        icon: 'trash',
        errorMessage: t('usage.clearFailed'),
        onConfirm: async () => {
          setClearing(true);
          try {
            await clearServiceUsage();
            await refreshServiceUsage();
            notify({ message: t('usage.cleared'), state: 'deleted' });
          } finally {
            setClearing(false);
          }
        },
      });
    } finally {
      clearPending.current = false;
    }
  };
  const clearControl = view === 'purpose' && serviceUsage.data && (
    <Btn
      className="ol-usage-clear-btn"
      icon="trash"
      disabled={clearing}
      onClick={() => void clear()}
      style={{ minHeight: 34, lineHeight: 1.4, flexShrink: 0, whiteSpace: 'nowrap' }}
    >
      {t('usage.clear')}
    </Btn>
  );
  const fillViewport = view === 'services';
  return (
    <div
      className="ol-usage-page"
      data-fill-viewport={fillViewport || undefined}
      style={{ width: '100%', minWidth: 0 }}
    >
      <PageHeader
        title={text('用量', 'Usage')}
        desc={text(
          '按服务与用途查看本机保留的使用记录。',
          'Review retained local records by service and purpose.',
        )}
      />
      <div className="ol-usage-controls">
        <nav aria-label={text('用量页面', 'Usage pages')} className="ol-usage-tabs">
          <Segmented
            compact
            label={text('用量页面', 'Usage pages')}
            options={[
              { value: 'overview', label: text('概览', 'Overview') },
              { value: 'purpose', label: text('用途', 'Purpose') },
              { value: 'services', label: text('服务', 'Services') },
            ]}
            value={view}
            onChange={setView}
          />
        </nav>
        <div className="ol-usage-toolbar">
          <Segmented
            compact
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
      </div>
      {!supportsServiceUsage() ? (
        <p className="ol-usage-banner">
          <Icon name="info" size={14} />
          <span>
            {text(
              '当前平台不提供本地用量账本。',
              'The local usage ledger is unavailable on this host.',
            )}
          </span>
        </p>
      ) : (
        serviceUsage.error !== null && (
          <p className="ol-usage-banner ol-usage-banner--alert" role="alert">
            <Icon name="alert" size={14} />
            <span>
              {text('无法读取本地 Token 账本。', 'Could not read the local token ledger.')}
            </span>
          </p>
        )
      )}
      {serviceUsage.data && serviceUsage.data.droppedOrFailed > 0 && (
        <p className="ol-usage-banner ol-usage-banner--alert" role="alert">
          <Icon name="alert" size={14} />
          <span>
            {text(
              '本次运行中部分用量未能保存，统计不完整。',
              'Some usage could not be saved during this run.',
            )}
          </span>
        </p>
      )}
      {history.error !== null && (
        <p className="ol-usage-banner ol-usage-banner--alert" role="alert">
          <Icon name="alert" size={14} />
          <span>{text('无法读取历史记录。', 'Could not load history.')}</span>
        </p>
      )}
      {history.data === null && history.loading && (
        <p className="ol-usage-banner" role="status">
          <span>{text('正在加载历史记录…', 'Loading history…')}</span>
        </p>
      )}
      {serviceUsage.data === null && serviceUsage.loading && (
        <p className="ol-usage-banner" role="status">
          <span>{text('正在加载本地账本…', 'Loading local ledger…')}</span>
        </p>
      )}
      {view === 'overview' && <UsageOverview dashboard={dashboard} />}
      {view === 'services' && <UsageServices dashboard={dashboard} />}
      {view === 'purpose' && <UsagePurpose dashboard={dashboard} />}
    </div>
  );
}
