import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { ActivityDay } from '../../api/contracts';
import { useActivityApps } from '../../data/history';
import { formatLocaleDecimal, formatLocaleNumber } from '../../lib/localeFormat';
import {
  GitHubActivity,
  type Contribution,
  type ContributionLevel,
} from '../../ui/controls/GitHubActivity';
import { Card } from '../../pages/_atoms';

function localDateKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function useActivityLabels() {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language || 'en';
  const labels = (key: string, english: string, chinese: string) =>
    t(`overview.${key}`, { defaultValue: locale.startsWith('zh') ? chinese : english });
  return { labels, locale, t };
}

export function ActivityHeatmap({ activity }: { activity: ActivityDay[] }) {
  const { labels, locale, t } = useActivityLabels();
  const year = new Date().getFullYear();
  const apps = useActivityApps(year);
  const appRows =
    apps.data?.status === 'available'
      ? apps.data.rows.slice(0, 5).map(([name, count]) => ({ name, count }))
      : [];
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

  return (
    <section className="ol-overview-heatmap" aria-label={t('overview.activityTitle')}>
      <div className="ol-overview-heatmap-row">
        <div className="ol-overview-calendar">
          <GitHubActivity
            year={year}
            cellSize={11}
            showMonths
            contributions={data.contributions}
            style={{ width: '100%' }}
            apps={appRows}
            accent="var(--ol-accent-solid-bg)"
            heading={`${t('overview.activityCount', { count: data.total })} · ${year}`}
            label={labels('activityAppsLabel', 'Top apps by dictations', '语音输入次数最多的软件')}
            showLabel={labels('activityAppsShow', 'Show top apps', '展开常用软件')}
            hideLabel={labels('activityAppsHide', 'Hide top apps', '收起常用软件')}
            monthNames={data.monthNames}
            formatDay={(day) =>
              `${dateFormat.format(new Date(`${day.date}T00:00:00`))} · ${t('overview.activityCount', { count: day.count })}`
            }
          />
          {apps.data?.status === 'unsupported' && (
            <p className="ol-overview-apps-status">
              {labels(
                'activityAppsUnavailable',
                'Yearly activity by app is unavailable on this host.',
                '当前平台不提供按软件汇总的年度活动。',
              )}
            </p>
          )}
          {apps.error !== null && (
            <p className="ol-overview-apps-status" role="status">
              {labels(
                'activityAppsError',
                'Could not load yearly activity by app.',
                '无法读取按软件汇总的年度活动。',
              )}
            </p>
          )}
        </div>
        <Card className="ol-overview-year" padding={14}>
          <h3>{labels('activityYearTitle', 'This year', '今年')}</h3>
          <dl className="ol-overview-year-stats">
            <div>
              <dd>{formatLocaleNumber(data.active, locale)}</dd>
              <dt>{labels('activityActiveDays', 'Active days', '活跃天数')}</dt>
            </div>
            <div>
              <dd>{formatLocaleNumber(data.peak, locale)}</dd>
              <dt>{labels('activityPeakDay', 'Busiest day', '单日最多')}</dt>
            </div>
            <div>
              <dd>{formatLocaleDecimal(data.average, locale)}</dd>
              <dt>{labels('activityDailyAverage', 'Daily average', '日均')}</dt>
            </div>
          </dl>
        </Card>
      </div>
    </section>
  );
}
