import { useEffect, useState, useSyncExternalStore } from 'react';
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
import { Button } from '../../ui/controls/Button';
import { Select } from '../../ui/controls/Select';
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

  const refreshCatalog = async () => {
    setRefreshing(true);
    try {
      await refreshAppStyleCatalog();
      await query.refresh();
      setCatalogError(false);
    } catch {
      setCatalogError(true);
    } finally {
      setRefreshing(false);
    }
  };

  useEffect(() => {
    let live = true;
    const stops: Array<() => void> = [];
    const refresh = () => {
      if (document.visibilityState === 'visible') void query.refresh();
    };
    const keep = async (source: Promise<() => void>) => {
      const stop = await source;
      if (live) stops.push(stop);
      else stop();
    };
    void Promise.allSettled([
      keep(listenHistoryChanges(refresh)),
      keep(listenStylePackChanges(() => packQuery.invalidate())),
    ]);
    void refreshCatalog();
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      live = false;
      stops.forEach((stop) => stop());
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, [query, packQuery]);

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
  const memberOptions = [...members.values()].sort((a, b) => a.name.localeCompare(b.name));

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

  return (
    <div className="ol-app-style-page">
      <PageHeader
        title={t('appStyles.title')}
        right={
          <Button disabled={refreshing} onClick={() => void refreshCatalog()}>
            {t('appStyles.refreshCatalog')}
          </Button>
        }
      />
      <p className="ol-app-style-hint">{t('appStyles.priority')}</p>
      {catalogError && (
        <p role="status" className="ol-app-style-hint">
          {t(
            routing?.catalog.categories.length
              ? 'appStyles.cachedCatalog'
              : 'appStyles.catalogUnavailable',
          )}
        </p>
      )}
      {(error || Boolean(readError)) && <p role="alert">{t('common.operationFailed')}</p>}
      <div className="ol-app-style-panels">
        <Card className="ol-app-style-panel">
          <h2>{t('appStyles.byCategory')}</h2>
          <div className="ol-app-style-panel-body">
            {routing?.catalog.categories.map((category) => (
              <section className="ol-app-style-group" key={category.id}>
                <h3>{categoryName(category)}</h3>
                <div className="ol-app-style-category-controls">
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
                </div>
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
          <div className="ol-app-style-panel-body">
            {[...(routing?.apps ?? [])]
              .sort((a, b) => a.app.name.localeCompare(b.app.name))
              .map(({ app, category, stylePackId }) => {
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
            {!routing?.apps.length && <p className="ol-app-style-hint">{t('appStyles.empty')}</p>}
          </div>
          <Button
            variant="ghost"
            disabled={busy || !routing?.apps.some((entry) => entry.seen)}
            onClick={async () => {
              if (
                await confirm({
                  title: t('appStyles.clearHistory'),
                  description: t('appStyles.clearConfirm'),
                  confirmLabel: t('appStyles.clearHistory'),
                  cancelLabel: t('common.cancel'),
                  tone: 'danger',
                })
              )
                await commit(clearInputAppHistory);
            }}
          >
            {t('appStyles.clearHistory')}
          </Button>
        </Card>
      </div>
    </div>
  );
}
