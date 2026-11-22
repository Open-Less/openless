import { useEffect, useId, useState } from 'react';
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
import { getStylePackPresentation } from '../../lib/stylePackPresentation';
import { Button } from '../../ui/controls/Button';
import { Select } from '../../ui/controls/Select';
import { Dialog, DialogBody, DialogFooter, DialogHeader } from '../../ui/overlays/Dialog';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import './app-style-routing.css';

const categories: readonly AppCategory[] = ['coding', 'chat', 'email'];

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
  const [routing, setRouting] = useState<AppStyleRouting | null>(null);
  const [icons, setIcons] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    let live = true;
    void getAppStyleRouting().then(
      (snapshot) => {
        if (live) setRouting(snapshot);
        if (snapshot.apps.length && live) {
          void getInputAppIcons().then(
            (result) => {
              if (live) setIcons(result);
            },
            () => {},
          );
        }
      },
      () => {
        if (live) setError(true);
      },
    );
    return () => {
      live = false;
    };
  }, []);

  const commit = async (write: () => Promise<AppStyleRouting>) => {
    setBusy(true);
    setError(false);
    try {
      setRouting(await write());
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

  function picker(
    assignment: AppStyleAssignment,
    label: string,
    inherit: string,
    owners: Map<string, string>,
  ) {
    const current = assignment.stylePackId ?? '';
    const options = [
      { value: '', label: inherit },
      ...packs
        .filter((pack) => pack.enabled || pack.id === current)
        .map((pack) => {
          const owner = pack.id === current ? undefined : owners.get(pack.id);
          return {
            value: pack.id,
            label: owner
              ? t('appStyles.assignedTo', { pack: packName(pack), owner })
              : pack.enabled
                ? packName(pack)
                : t('appStyles.unavailablePack', { pack: packName(pack) }),
            disabled: Boolean(owner),
          };
        }),
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

  const categoryOwners = new Map<string, string>();
  const appOwners = new Map<string, string>();
  for (const category of categories) {
    const pack = routing?.categories[category];
    if (pack) categoryOwners.set(pack, t(`appStyles.categories.${category}`));
  }
  for (const entry of routing?.apps ?? []) {
    if (entry.stylePackId) appOwners.set(entry.stylePackId, entry.app.name);
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
            {([...categories, null] as const).map((category) => {
              const label = t(`appStyles.categories.${category ?? 'other'}`);
              const apps = routing.apps
                .filter((entry) => entry.category === category)
                .sort((a, b) => a.app.name.localeCompare(b.app.name));
              if (!category && !apps.length) return null;
              const categoryPack = category
                ? packs.find((pack) => pack.enabled && pack.id === routing.categories[category])
                : undefined;
              const defaultLabel = t('appStyles.useCurrent');
              const inherited = categoryPack
                ? t('appStyles.inherit', { pack: packName(categoryPack) })
                : defaultLabel;
              return (
                <section className="ol-app-style-group" key={category ?? 'other'}>
                  <div className="ol-app-style-row">
                    <h3>{label}</h3>
                    {category &&
                      picker(
                        {
                          target: 'category',
                          category,
                          stylePackId: routing.categories[category] ?? null,
                        },
                        label,
                        defaultLabel,
                        categoryOwners,
                      )}
                  </div>
                  {apps.map(({ app, stylePackId }) => (
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
                        <span title={app.name}>{app.name}</span>
                      </span>
                      {picker(
                        { target: 'app', appId: app.id, stylePackId },
                        app.name,
                        inherited,
                        appOwners,
                      )}
                    </div>
                  ))}
                </section>
              );
            })}
            {!routing.apps.length && <p className="ol-app-style-hint">{t('appStyles.empty')}</p>}
          </>
        ) : (
          !error && <p role="status">{t('common.loading')}</p>
        )}
      </DialogBody>
      {error && (
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
