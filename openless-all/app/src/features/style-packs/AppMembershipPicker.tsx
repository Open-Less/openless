import { Popover } from '@base-ui/react/popover';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { TextField } from '../../ui/controls/TextField';
import { usePopupViewport } from '../../ui/layout/AppViewport';

export interface AppMemberOption {
  id: string;
  name: string;
  category: string | null;
}

export function AppMembershipPicker({
  category,
  categoryName,
  options,
  busy,
  onToggle,
}: {
  category: string;
  categoryName: string;
  options: AppMemberOption[];
  busy: boolean;
  onToggle: (app: AppMemberOption) => Promise<void>;
}) {
  const { t } = useTranslation();
  const { boundary, container } = usePopupViewport();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const selected = options.filter((app) => app.category === category).length;
  const visible = options.filter((app) =>
    app.name.toLocaleLowerCase().includes(search.toLocaleLowerCase()),
  );
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger
        className="ol-select-trigger"
        disabled={busy}
        aria-label={t('appStyles.appsFor', { name: categoryName })}
      >
        <span className="ol-select-value">{t('appStyles.selectedApps', { count: selected })}</span>
        <Icon name="chevDown" size={11} />
      </Popover.Trigger>
      <Popover.Portal container={container} className="ol-popup-portal">
        <Popover.Positioner
          className="ol-select-positioner"
          positionMethod="fixed"
          collisionBoundary={boundary}
          side="bottom"
          align="start"
          sideOffset={8}
          collisionPadding={8}
        >
          <Popover.Popup
            className="ol-select-popup ol-app-members-popup"
            aria-label={t('appStyles.appsFor', { name: categoryName })}
          >
            <TextField
              aria-label={t('appStyles.searchApps')}
              placeholder={t('appStyles.searchApps')}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            <div className="ol-app-members-list" role="group" aria-label={categoryName}>
              {visible.map((app) => {
                const checked = app.category === category;
                const conflict = app.category !== null && !checked;
                return (
                  <button
                    key={app.id}
                    type="button"
                    role="checkbox"
                    aria-checked={checked}
                    aria-label={app.name}
                    disabled={busy}
                    className="ol-app-member-option"
                    data-conflict={conflict || undefined}
                    onClick={() => {
                      if (conflict) setOpen(false);
                      void onToggle(app);
                    }}
                  >
                    <span className="ol-app-member-check" aria-hidden="true">
                      {(checked || conflict) && (
                        <Icon name={conflict ? 'close' : 'check'} size={12} />
                      )}
                    </span>
                    <span>{app.name}</span>
                  </button>
                );
              })}
              {!visible.length && <p className="ol-app-style-hint">{t('appStyles.noApps')}</p>}
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
