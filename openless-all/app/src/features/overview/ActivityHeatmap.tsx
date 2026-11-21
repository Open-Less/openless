import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ActivityDay } from '../../api/contracts';
import { getActivityAppIcons } from '../../api/history';
import { useActivityApps } from '../../data/history';
import { getRuntimeEnvironment } from '../../runtime/environment';
import { formatLocaleDecimal, formatLocaleNumber } from '../../lib/localeFormat';
import {
  GitHubActivity,
  type Contribution,
  type ContributionLevel,
} from '../../ui/controls/GitHubActivity';
import './overview.css';

function localDateKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

export function ActivityHeatmap({
  activity,
  mobile = false,
}: {
  activity: ActivityDay[];
  mobile?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language || 'en';
  const year = new Date().getFullYear();
  const apps = useActivityApps(year);
  const appRows = useMemo(
    () =>
      apps.data?.status === 'available'
        ? [...apps.data.rows]
            .sort(([, left], [, right]) => right - left)
            .slice(0, 5)
            .map(([name, count]) => ({ name, count }))
        : [],
    [apps.data],
  );
  const [icons, setIcons] = useState<Record<string, string>>({});
  const { host, os, isMobile } = getRuntimeEnvironment();
  const iconKey =
    host === 'tauri' && !isMobile && (os === 'mac' || os === 'win') && appRows.length > 0
      ? JSON.stringify(appRows.map(({ name }) => name).sort())
      : null;
  useEffect(() => {
    if (iconKey === null) return;
    let active = true;
    void getActivityAppIcons(year)
      .then((next) => {
        if (active) setIcons(next);
      })
      .catch(() => {
        // Missing icons (including older hosts) must never hide the ranking.
      });
    return () => {
      active = false;
    };
  }, [iconKey, year]);
  const data = useMemo(() => {
    const first = new Date(year, 0, 1);
    const last = new Date(year, 11, 31);
    const start = new Date(first);
    start.setDate(start.getDate() - start.getDay());
    const end = new Date(last);
    end.setDate(end.getDate() + (6 - end.getDay()));
    const counts = new Map(activity.map(({ date, count }) => [date, count]));
    const monthFormat = new Intl.DateTimeFormat(locale, { month: 'short' });
    const contributions: { date: string; count: number }[] = [];
    for (const day = new Date(start); day <= end; day.setDate(day.getDate() + 1)) {
      const date = localDateKey(day);
      contributions.push({
        date,
        count: day >= first && day <= last ? (counts.get(date) ?? 0) : 0,
      });
    }
    const total = contributions.reduce((sum, day) => sum + day.count, 0);
    const peak = contributions.reduce((max, day) => Math.max(max, day.count), 0);
    const active = contributions.filter((day) => day.count > 0).length;
    const daysInYear = (Date.UTC(year + 1, 0, 1) - Date.UTC(year, 0, 1)) / 86_400_000;
    return {
      contributions: contributions.map<Contribution>((day) => ({
        ...day,
        level: (day.count > 0 && peak > 0
          ? Math.ceil((day.count / peak) * 4)
          : 0) as ContributionLevel,
      })),
      total,
      peak,
      active,
      average: total / daysInYear,
      monthNames: Array.from({ length: 12 }, (_, month) =>
        monthFormat.format(new Date(year, month, 1)),
      ),
    };
  }, [activity, locale, year]);
  const dateFormat = useMemo(
    () => new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }),
    [locale],
  );

  const appStatus =
    apps.error !== null
      ? t('overview.activityAppsError')
      : apps.data?.status === 'unsupported'
        ? t('overview.activityAppsUnavailable')
        : null;

  return (
    <section className="ol-overview-heatmap" aria-label={t('overview.activityTitle')}>
      <GitHubActivity
        className="ol-overview-activity"
        style={{ width: '100%' }}
        summary={
          <aside className="ol-overview-year" aria-label={t('overview.activityYearTitle')}>
            <p className="ol-overview-year-title">{t('overview.activityYearTitle')}</p>
            <dl className="ol-overview-year-stats">
              <div className="ol-overview-year-stat">
                <dt className="ol-overview-year-label">{t('overview.activityActiveDays')}</dt>
                <dd className="ol-overview-year-value">
                  {formatLocaleNumber(data.active, locale)}
                </dd>
              </div>
              <div className="ol-overview-year-stat">
                <dt className="ol-overview-year-label">{t('overview.activityPeakDay')}</dt>
                <dd className="ol-overview-year-value">{formatLocaleNumber(data.peak, locale)}</dd>
              </div>
              <div className="ol-overview-year-stat">
                <dt className="ol-overview-year-label">{t('overview.activityDailyAverage')}</dt>
                <dd className="ol-overview-year-value">
                  {formatLocaleDecimal(data.average, locale)}
                </dd>
              </div>
            </dl>
          </aside>
        }
        footer={
          appStatus && (
            <p className="ol-overview-apps-status" role="status">
              {appStatus}
            </p>
          )
        }
        inFlow={mobile}
        year={year}
        cellSize={11}
        fitToWidth
        showMonths
        contributions={data.contributions}
        apps={appRows.map((row) => ({ ...row, icon: icons[row.name] }))}
        accent="var(--ol-accent-solid-bg)"
        heading={`${t('overview.activityCount', { count: data.total })} · ${year}`}
        label={t('overview.activityAppsLabel')}
        showLabel={t('overview.activityAppsShow')}
        hideLabel={t('overview.activityAppsHide')}
        monthNames={data.monthNames}
        formatDay={(day) =>
          `${dateFormat.format(new Date(`${day.date}T00:00:00`))} · ${t('overview.activityCount', { count: day.count })}`
        }
      />
    </section>
  );
}
