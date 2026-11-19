import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { clearServiceUsage, supportsServiceUsage } from '../../api/usage';
import { historyStore, useHistoryRecords } from '../../data/history';
import { refreshServiceUsage, useServiceUsage } from '../../data/usage';
import { Segmented } from '../../ui/controls/Segmented';
import { Button as Btn } from '../../ui/controls/Button';
import { PageHeader } from '../../ui/layout/PageHeader';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { buildUsageDashboard } from './model';
import { useUsageLabels } from './labels';
import { UsageOverview, UsageServices, UsageHistory } from './UsageViews';
import './usage.css';

export function Usage() {
  const { t } = useTranslation();
  const { text } = useUsageLabels();
  const { confirm, notify } = useInteraction('usage');
  const history = useHistoryRecords();
  const serviceUsage = useServiceUsage();
  const [days, setDays] = useState(30);
  const [view, setView] = useState<'overview' | 'services' | 'history'>('overview');
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

  const refresh = async () => {
    if (clearing) return;
    await Promise.all([historyStore.refreshRecords(), refreshServiceUsage()]);
  };
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
  const clearControl = view === 'history' && serviceUsage.data && (
    <Btn
      style={{ minHeight: 36, lineHeight: 1.4, flexShrink: 0, whiteSpace: 'nowrap' }}
      disabled={clearing}
      onClick={() => void clear()}
    >
      {t('usage.clear')}
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
      <nav aria-label={text('用量页面', 'Usage pages')} className="ol-usage-tabs">
        <Segmented
          label={text('用量页面', 'Usage pages')}
          options={[
            { value: 'overview', label: text('概览', 'Overview') },
            { value: 'services', label: text('服务与用途', 'Services and purposes') },
            { value: 'history', label: text('记录', 'History') },
          ]}
          value={view}
          onChange={setView}
        />
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
      {!supportsServiceUsage() ? (
        <p>
          {text(
            '当前平台不提供本地用量账本。',
            'The local usage ledger is unavailable on this host.',
          )}
        </p>
      ) : (
        serviceUsage.error !== null && (
          <p role="alert">
            {text('无法读取本地 Token 账本。', 'Could not read the local token ledger.')}
          </p>
        )
      )}
      {serviceUsage.data && serviceUsage.data.droppedOrFailed > 0 && (
        <p role="alert">
          {text(
            '本次运行中部分用量未能保存，统计不完整。',
            'Some usage could not be saved during this run.',
          )}
        </p>
      )}
      {history.error !== null && (
        <p role="alert">{text('无法读取历史记录。', 'Could not load history.')}</p>
      )}
      {history.data === null && history.loading && (
        <p role="status">{text('正在加载历史记录…', 'Loading history…')}</p>
      )}
      {serviceUsage.data === null && serviceUsage.loading && (
        <p role="status">{text('正在加载本地账本…', 'Loading local ledger…')}</p>
      )}
      {view === 'overview' && <UsageOverview dashboard={dashboard} />}
      {view === 'services' && <UsageServices dashboard={dashboard} />}
      {view === 'history' && <UsageHistory dashboard={dashboard} />}
    </div>
  );
}
