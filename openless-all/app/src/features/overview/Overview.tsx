import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession } from '../../api/contracts';
import type { ProviderKind } from '../../api/providers';
import type { OverviewSettingsSection } from '../../lib/overviewSetup';
import { historyStore, useActivityStats, useHistoryRecords } from '../../data/history';
import { useHotkeySettings } from '../../data/preferences';
import { getRuntimeEnvironment, getShortcutPlatform } from '../../runtime/environment';
import {
  buildPeriodSeries,
  type ActivityPeriod,
  type ActivityMetric,
} from '../../lib/activityMetrics';
import { HarmonyDictation } from '../dictation/HarmonyDictation';
import { useMobileLayout } from '../../lib/useMobileLayout';
import { PageHeader } from '../../ui/layout/PageHeader';
import { useModalActive } from '../../ui/layout/AppViewport';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { PeriodMetricsCard } from './PeriodMetricsCard';
import { ActivityHeatmap } from './ActivityHeatmap';
import { TodayMetrics } from './TodayMetrics';
import { RecentHistory } from './RecentHistory';
import { OverviewServiceStatus, useOverviewServices } from './ServiceStatus';
import './overview.css';

interface OverviewProps {
  onOpenHistory?: () => void;
  onOpenSettings?: (section: OverviewSettingsSection, serviceView?: ProviderKind) => void;
}

const emptyHistory: DictationSession[] = [];

export function Overview({ onOpenHistory, onOpenSettings }: OverviewProps) {
  const { t } = useTranslation();
  const { notify } = useInteraction('overview');
  const modalActive = useModalActive();
  const { prefs } = useHotkeySettings();
  const runtime = getRuntimeEnvironment();
  const mobile = useMobileLayout();
  const { isMac } = getShortcutPlatform();
  const history = useHistoryRecords();
  const activity = useActivityStats();
  const services = useOverviewServices();
  const refreshing = useRef(false);
  const [period, setPeriod] = useState<ActivityPeriod>(7);
  const [metric, setMetric] = useState<ActivityMetric>('count');
  const series = useMemo(
    () => buildPeriodSeries(activity.data ?? [], period, metric),
    [activity.data, period, metric],
  );
  const refreshServices = services.refresh;

  const refreshAll = useCallback(async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      const [snapshots, credentials] = await Promise.all([
        historyStore.refresh(),
        refreshServices(),
      ]);
      if (credentials === 'stale') return;
      const ok = credentials === 'ok' && snapshots.every((snapshot) => snapshot.error === null);
      notify(
        ok
          ? { message: t('common.refreshed'), state: 'saved' }
          : { message: t('common.operationFailed'), state: 'failed', durationMs: 3500 },
      );
    } finally {
      refreshing.current = false;
    }
  }, [notify, refreshServices, t]);

  useEffect(() => {
    if (modalActive) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.altKey || event.shiftKey) return;
      if ((isMac ? event.metaKey : event.ctrlKey) && event.key.toLowerCase() === 'r') {
        event.preventDefault();
        void refreshAll();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isMac, modalActive, refreshAll]);

  return (
    <div className="ol-overview-dashboard" data-mobile={mobile || undefined}>
      <PageHeader
        compact
        title={t('overview.title')}
        right={
          services.setup.providers.length > 0 ? (
            <OverviewServiceStatus services={services} onOpenSettings={onOpenSettings} />
          ) : (
            <div role="status" className="ol-overview-service-status">
              {t(services.loading ? 'overview.statusLoading' : 'overview.credentialsLoadError')}
            </div>
          )
        }
      />
      {runtime.host === 'harmony' && <HarmonyDictation onComplete={historyStore.invalidate} />}
      <TodayMetrics
        mobile={mobile}
        history={history.data ?? emptyHistory}
        historyError={history.error !== null && history.data === null}
      />
      <section
        className="ol-grid-12 ol-grid-fill ol-overview-lower"
        data-columns={mobile ? '1' : undefined}
      >
        <PeriodMetricsCard
          series={series}
          period={period}
          metric={metric}
          onPeriodChange={setPeriod}
          onMetricChange={setMetric}
          loadError={activity.error !== null && activity.data === null}
          onRetry={() => void historyStore.refreshActivity()}
        />
        <RecentHistory
          history={history.data ?? emptyHistory}
          historyError={history.error !== null}
          loading={history.data === null && history.loading}
          onOpenHistory={onOpenHistory}
          onRetry={() => void historyStore.refreshRecords()}
        />
      </section>
      {!runtime.isMobile && prefs?.showOverviewActivityHeatmap !== false && activity.data && (
        <ActivityHeatmap activity={activity.data} mobile={mobile} />
      )}
    </div>
  );
}
