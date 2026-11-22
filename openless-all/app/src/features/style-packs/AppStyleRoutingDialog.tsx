import { useEffect, useId, useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  AppCategory,
  AppStyleAssignment,
  AppStyleRouting,
  StylePack,
} from '../../api/contracts';
import {
  clearInputAppHistory,
  getAppStyleRouting,
  getInputAppIcons,
  setAppStyleAssignment,
} from '../../api/style-packs';
import { SnapshotQuery } from '../../data/snapshot-query';
import { listenHistoryChanges } from '../../runtime/domain-events';
import { getStylePackPresentation } from '../../lib/stylePackPresentation';
import { Button } from '../../ui/controls/Button';
import { Select } from '../../ui/controls/Select';
import { Dialog, DialogBody, DialogFooter, DialogHeader } from '../../ui/overlays/Dialog';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import './app-style-routing.css';

const categories: readonly AppCategory[] = [
  'coding',
  'chat',
  'email',
  'writing',
  'office',
  'browser',
];

export function AppStyleRoutingDialog({
  packs,
  onClose,
}: {
  packs: StylePack[];
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { confirm } = useInteraction('app-style-routing');
  const title = useId();
  const [query] = useState(() => new SnapshotQuery(getAppStyleRouting, async () => {}));
  const { data: routing, error: loadError } = useSyncExternalStore(
    query.subscribe,
    query.getSnapshot,
  );
  const [icons, setIcons] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | undefined;
    const refresh = () => {
      if (document.visibilityState === 'visible') void query.refresh();
    };
    void listenHistoryChanges(refresh).then(
      (stop) => {
        if (live) unlisten = stop;
        else stop();
      },
      () => {
        if (live) setError(true);
      },
    );
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      live = false;
      unlisten?.();
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, [query]);

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

  const commit = async (write: () => Promise<AppStyleRouting>) => {
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

  const packName = (pack: StylePack) => getStylePackPresentation(pack, t).name;

  function picker(assignment: AppStyleAssignment, label: string, inherit: string) {
    const current = assignment.stylePackId ?? '';
    const options = [
      { value: '', label: inherit },
      ...packs
        .filter((pack) => pack.enabled || pack.id === current)
        .map((pack) => ({
          value: pack.id,
          label: pack.enabled
            ? packName(pack)
            : t('appStyles.unavailablePack', { pack: packName(pack) }),
        })),
    ];
    if (current && !packs.some((pack) => pack.id === current)) {
      options.push({ value: current, label: t('appStyles.missingPack') });
    }
    return (
      <div className="ol-app-style-picker">
        <Select
          ariaLabel={t('appStyles.selectFor', { name: label })}
          options={options}
          popupClassName="ol-app-style-options"
          value={current}
          disabled={busy}
          onChange={(value) =>
            void commit(() =>
              setAppStyleAssignment({
                ...assignment,
                stylePackId: value || null,
              }),
            )
          }
        />
      </div>
    );
  }

  return (
    <Dialog
      labelledBy={title}
      onClose={() => {
        if (!busy) onClose();
      }}
      width="min(640px, 100%)"
      cardClassName="ol-app-style-dialog"
    >
      <DialogHeader>
        <h2 id={title}>{t('appStyles.title')}</h2>
        <Button
          icon="close"
          ariaLabel={t('common.close')}
          style={{
            display: 'grid',
            placeItems: 'center',
            flex: '0 0 30px',
            width: 30,
            height: 30,
            padding: 0,
            borderRadius: '50%',
          }}
          disabled={busy}
          onClick={onClose}
        />
      </DialogHeader>
      <DialogBody>
        {routing ? (
          <>
            <p className="ol-app-style-hint">{t('appStyles.priority')}</p>
            <section className="ol-app-style-group">
              <h3>{t('appStyles.byCategory')}</h3>
              {categories.map((category) => {
                const label = t(`appStyles.categories.${category}`);
                return (
                  <div className="ol-app-style-row" key={category}>
                    <span className="ol-app-style-name">{label}</span>
                    {picker(
                      {
                        target: 'category',
                        category,
                        stylePackId: routing.categories[category] ?? null,
                      },
                      label,
                      t('appStyles.useCurrent'),
                    )}
                  </div>
                );
              })}
            </section>
            {routing.apps.length > 0 && (
              <section className="ol-app-style-group">
                <h3>{t('appStyles.byApp')}</h3>
                {[...routing.apps]
                  .sort((a, b) => a.app.name.localeCompare(b.app.name))
                  .map(({ app, category, stylePackId }) => {
                    const categoryPack = category
                      ? packs.find(
                          (pack) => pack.enabled && pack.id === routing.categories[category],
                        )
                      : undefined;
                    const inherited = categoryPack
                      ? t('appStyles.inherit', { pack: packName(categoryPack) })
                      : t('appStyles.useCurrent');
                    return (
                      <div className="ol-app-style-row ol-app-style-row--app" key={app.id}>
                        <span className="ol-app-style-name">
                          <span className="ol-app-style-icon" aria-hidden="true">
                            {icons[app.id] && (
                              <img
                                alt=""
                                src={icons[app.id]}
                                onError={(event) => {
                                  event.currentTarget.hidden = true;
                                }}
                              />
                            )}
                          </span>
                          <span title={app.name}>
                            {app.name}
                            {category && <small>{t(`appStyles.categories.${category}`)}</small>}
                          </span>
                        </span>
                        {picker({ target: 'app', appId: app.id, stylePackId }, app.name, inherited)}
                      </div>
                    );
                  })}
              </section>
            )}
            {!routing.apps.length && <p className="ol-app-style-hint">{t('appStyles.empty')}</p>}
          </>
        ) : (
          !error && !loadError && <p role="status">{t('common.loading')}</p>
        )}
      </DialogBody>
      {(error || Boolean(loadError)) && (
        <p className="ol-dialog-error" role="alert">
          {t('common.operationFailed')}
        </p>
      )}
      <DialogFooter>
        <Button
          variant="ghost"
          disabled={busy || !routing?.apps.some((entry) => entry.seen)}
          onClick={() => void clearHistory()}
        >
          {t('appStyles.clearHistory')}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
