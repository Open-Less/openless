import { useEffect, useMemo, useState } from 'react';
import { getActivityAppIcons } from '../../api/history';
import { useActivityApps } from '../../data/history';
import { getRuntimeEnvironment } from '../../runtime/environment';

export function useActivityAppRanking(year: number) {
  const apps = useActivityApps(year);
  const rows = useMemo(
    () =>
      apps.data?.status === 'available'
        ? [...apps.data.rows]
            .sort(([, left], [, right]) => right - left)
            .slice(0, 5)
            .map(([name, count]) => ({ name, count }))
        : [],
    [apps.data],
  );
  const { host, os, isMobile } = getRuntimeEnvironment();
  const key =
    host === 'tauri' && !isMobile && (os === 'mac' || os === 'win') && rows.length > 0
      ? JSON.stringify([year, rows.map(({ name }) => name).sort()])
      : null;
  const [resolved, setResolved] = useState<{ key: string; icons: Record<string, string> }>();
  useEffect(() => {
    if (key === null) return;
    let active = true;
    void getActivityAppIcons(year)
      .then((icons) => {
        if (active) setResolved({ key, icons });
      })
      .catch(() => {
        // Missing icons (including older hosts) must never hide the ranking.
      });
    return () => {
      active = false;
    };
  }, [key, year, apps.data]);
  const icons = resolved?.icons;
  return {
    rows: rows.map((row) => ({ ...row, icon: icons?.[row.name] })),
    loadFailed: apps.error !== null,
    unsupported: apps.data?.status === 'unsupported',
  };
}
