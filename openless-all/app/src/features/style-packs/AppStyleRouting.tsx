import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  AppStyleAssignment,
  AppStyleRouting as Routing,
  CatalogCategory,
  StylePack,
} from '../../api/contracts';
import {
  clearInputAppHistory,
  getAppStyleRouting,
  getInputAppIcons,
  listStylePacks,
  listenStylePackChanges,
  refreshAppStyleCatalog,
  setAppStyleAssignment,
} from '../../api/style-packs';
import { SnapshotQuery } from '../../data/snapshot-query';
import { getStylePackPresentation } from '../../lib/stylePackPresentation';
import { listenHistoryChanges } from '../../runtime/domain-events';
import { getShortcutPlatform } from '../../runtime/environment';
import { listenMainWindowFocus } from '../../runtime/windows/mainWindowFocus';
import { Button } from '../../ui/controls/Button';
import { Select } from '../../ui/controls/Select';
import { TextField } from '../../ui/controls/TextField';
import { Card } from '../../ui/layout/Card';
import { PageHeader } from '../../ui/layout/PageHeader';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { AppMembershipPicker, type AppMemberOption } from './AppMembershipPicker';
import './app-style-routing.css';

export function AppStyleRouting() {
  const { t, i18n } = useTranslation();
  const { confirm } = useInteraction('app-style-routing');
  const [query] = useState(() => new SnapshotQuery(getAppStyleRouting, async () => {}));
  const [packQuery] = useState(() => new SnapshotQuery(listStylePacks, async () => {}));
  const { data: routing, error: readError } = useSyncExternalStore(
    query.subscribe,
    query.getSnapshot,
  );
  const { data: packs = null } = useSyncExternalStore(packQuery.subscribe, packQuery.getSnapshot);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [catalogError, setCatalogError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [icons, setIcons] = useState<Record<string, string>>({});
  const catalogRequest = useRef<Promise<void> | null>(null);
  const [appSearch, setAppSearch] = useState('');

  const refreshCatalog = useCallback(() => {
    if (catalogRequest.current) return catalogRequest.current;
    setRefreshing(true);
    catalogRequest.current = (async () => {
      try {
        await refreshAppStyleCatalog();
        await query.refresh();
        setCatalogError(false);
      } catch {
        setCatalogError(true);
      } finally {
        setRefreshing(false);
        catalogRequest.current = null;
      }
    })();
    return catalogRequest.current;
  }, [query]);

  useEffect(() => {
    let live = true;
    const stops: Array<() => void> = [];
    const refresh = () => {
      if (document.visibilityState === 'visible') void query.refresh();
    };
    const renewCatalog = () => {
      if (document.visibilityState === 'visible') {
        refresh();
        void refreshCatalog();
      }
    };
    const keep = async (source: Promise<() => void>) => {
      const stop = await source;
      if (live) stops.push(stop);
      else stop();
    };
    void Promise.allSettled([
      keep(listenHistoryChanges(refresh)),
      keep(listenStylePackChanges(() => packQuery.invalidate())),
      keep(listenMainWindowFocus(renewCatalog)),
    ]);
    renewCatalog();
    window.addEventListener('focus', renewCatalog);
    document.addEventListener('visibilitychange', renewCatalog);
    return () => {
      live = false;
      stops.forEach((stop) => stop());
      window.removeEventListener('focus', renewCatalog);
      document.removeEventListener('visibilitychange', renewCatalog);
    };
  }, [query, packQuery, refreshCatalog]);

  const apps = (routing?.apps ?? [])
    .filter(({ app }) =>
      app.name.toLocaleLowerCase().includes(appSearch.trim().toLocaleLowerCase()),
    )
    .sort((a, b) =>
      a.app.name.localeCompare(b.app.name, i18n.resolvedLanguage ?? i18n.language, {
        sensitivity: 'base',
        numeric: true,
      }),
    );

  const appIds =
    routing?.apps
      .map(({ app }) => app.id)
      .sort()
      .join('\n') ?? '';
  useEffect(() => {
    if (!appIds) return;
    let live = true;
    void getInputAppIcons().then(
      (result) => {
        if (live) setIcons(result);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [appIds]);

  const commit = async (write: () => Promise<Routing>) => {
    setBusy(true);
    setError(false);
    try {
      await write();
      await query.refresh();
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  };
  const packName = (pack: StylePack) => getStylePackPresentation(pack, t).name;
  const categoryName = (category: CatalogCategory) =>
    category.labels?.[i18n.resolvedLanguage ?? i18n.language] ?? category.name;
  const categoryLabel = (id: string) => {
    const category = routing?.catalog.categories.find((item) => item.id === id);
    return category ? categoryName(category) : id;
  };

  const picker = (
    assignment: Exclude<AppStyleAssignment, { target: 'membership' }>,
    label: string,
    inherit: string,
  ) => {
    const current = assignment.stylePackId ?? '';
    const options = [
      { value: '', label: inherit },
      ...(packs ?? [])
        .filter((pack) => pack.enabled || pack.id === current)
        .map((pack) => ({
          value: pack.id,
          label: pack.enabled
            ? packName(pack)
            : t('appStyles.unavailablePack', { pack: packName(pack) }),
        })),
    ];
    if (current && !packs?.some((pack) => pack.id === current))
      options.push({ value: current, label: t('appStyles.missingPack') });
    return (
      <Select
        ariaLabel={t('appStyles.selectFor', { name: label })}
        options={options}
        value={current}
        disabled={busy}
        popupClassName="ol-app-style-options"
        onChange={(value) =>
          void commit(() => setAppStyleAssignment({ ...assignment, stylePackId: value || null }))
        }
      />
    );
  };

  const members = new Map<string, AppMemberOption>();
  const platform = getShortcutPlatform();
  for (const client of routing?.catalog.clients ?? []) {
    if (client.canonicalClientId) continue;
    const ids = client.identifiers;
    const identifiable = platform.isMac
      ? ids.macosBundleIds.length ||
        (ids.macosBundlePrefixes?.length && ids.installedWebAppNames.length)
      : platform.isWindows && ids.windowsExecutableStems.length;
    if (!identifiable) continue;
    const id = `catalog:${client.id}`;
    const category =
      routing && Object.prototype.hasOwnProperty.call(routing.memberships, id)
        ? routing.memberships[id]
        : client.category;
    members.set(id, { id, name: client.name, category });
  }
  for (const entry of routing?.apps ?? [])
    members.set(entry.memberId, {
      id: entry.memberId,
      name: entry.app.name,
      category: entry.category,
    });
  const memberOptions = [...members.values()];

  const toggleMember = async (app: AppMemberOption, category: string) => {
    if (app.category && app.category !== category) {
      const pack = packs?.find((pack) => pack.id === routing?.categories[app.category!]);
      if (
        !(await confirm({
          title: t('appStyles.moveApp'),
          description: t('appStyles.moveConfirm', {
            app: app.name,
            category: categoryLabel(app.category),
            pack: pack ? packName(pack) : t('appStyles.useCurrent'),
            target: categoryLabel(category),
          }),
          confirmLabel: t('appStyles.moveHere'),
          cancelLabel: t('common.cancel'),
        }))
      )
        return;
    }
    await commit(() =>
      setAppStyleAssignment({
        target: 'membership',
        memberId: app.id,
        category: app.category === category ? null : category,
      }),
    );
  };

  const clearHistory = async () => {
    if (
      await confirm({
        title: t('appStyles.clearHistory'),
        description: t('appStyles.clearConfirm'),
        confirmLabel: t('appStyles.clearHistory'),
        cancelLabel: t('common.cancel'),
        tone: 'danger',
      })
    ) {
      await commit(clearInputAppHistory);
    }
  };

  return (
    <div className="ol-app-style-page">
      <PageHeader
        compact
        title={t('appStyles.title')}
        right={
          <Button
            disabled={busy || !routing?.apps.some((entry) => entry.seen)}
            onClick={() => void clearHistory()}
          >
            {t('appStyles.clearHistory')}
          </Button>
        }
      />
      <p
        className="ol-app-style-hint ol-app-style-status"
        role={error || readError ? 'alert' : catalogError ? 'status' : undefined}
      >
        {error || readError
          ? t('common.operationFailed')
          : catalogError
            ? t(
                routing?.catalog.categories.length
                  ? 'appStyles.cachedCatalog'
                  : 'appStyles.catalogUnavailable',
              )
            : t('appStyles.priority')}
      </p>
      <div className="ol-app-style-panels">
        <Card className="ol-app-style-panel">
          <h2>{t('appStyles.byCategory')}</h2>
          <div
            className="ol-app-style-panel-body ol-app-style-categories"
            style={{
              gridTemplateRows: `repeat(${routing?.catalog.categories.length || 1}, minmax(0, 1fr))`,
            }}
          >
            {routing?.catalog.categories.map((category) => (
              <section className="ol-app-style-group" key={category.id}>
                <AppMembershipPicker
                  category={category.id}
                  categoryName={categoryName(category)}
                  options={memberOptions}
                  busy={busy}
                  onToggle={(app) => toggleMember(app, category.id)}
                />
                {picker(
                  {
                    target: 'category',
                    category: category.id,
                    stylePackId: routing.categories[category.id] ?? null,
                  },
                  categoryName(category),
                  t('appStyles.useCurrent'),
                )}
              </section>
            ))}
            {!routing?.catalog.categories.length && (
              <p role="status">
                {t(refreshing ? 'common.loading' : 'appStyles.catalogUnavailable')}
              </p>
            )}
          </div>
        </Card>
        <Card className="ol-app-style-panel">
          <h2>{t('appStyles.byApp')}</h2>
          <div className="ol-app-style-app-search">
            <TextField
              aria-label={t('appStyles.searchApps')}
              placeholder={t('appStyles.searchApps')}
              value={appSearch}
              onChange={(event) => setAppSearch(event.target.value)}
            />
          </div>
          <div className="ol-app-style-panel-body ol-app-style-apps">
            {apps.map(({ app, category, stylePackId }) => {
              const categoryPack = category
                ? packs?.find((pack) => pack.enabled && pack.id === routing?.categories[category])
                : undefined;
              const inherited = categoryPack
                ? t('appStyles.inherit', { pack: packName(categoryPack) })
                : t('appStyles.useCurrent');
              return (
                <div className="ol-app-style-row ol-app-style-row--app" key={app.id}>
                  <span className="ol-app-style-name">
                    {icons[app.id] && (
                      <img
                        className="ol-app-style-icon"
                        alt=""
                        src={icons[app.id]}
                        onError={(event) => {
                          event.currentTarget.hidden = true;
                        }}
                      />
                    )}
                    <span title={app.name}>
                      {app.name}
                      {category && <small>{categoryLabel(category)}</small>}
                    </span>
                  </span>
                  {picker({ target: 'app', appId: app.id, stylePackId }, app.name, inherited)}
                </div>
              );
            })}
            {!apps.length && (
              <p className="ol-app-style-hint">
                {t(appSearch.trim() ? 'appStyles.noApps' : 'appStyles.empty')}
              </p>
            )}
          </div>
        </Card>
      </div>
    </div>
  );
}
