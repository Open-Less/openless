import { useId, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { Button } from '../../ui/controls/Button';
import { TextField } from '../../ui/controls/TextField';
import { Dialog, DialogBody, DialogFooter, DialogHeader } from '../../ui/overlays/Dialog';

const iconButtonStyle: CSSProperties = {
  display: 'grid',
  placeItems: 'center',
  padding: 0,
  width: 30,
  height: 30,
};

export function AppStylePagination({
  page,
  pages,
  onChange,
  disabled = false,
}: {
  page: number;
  pages: number;
  onChange: (page: number) => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="ol-app-style-pagination">
      <Button
        icon="chevLeft"
        style={iconButtonStyle}
        ariaLabel={t('appStyles.previousPage')}
        disabled={disabled || page === 0}
        onClick={() => onChange(page - 1)}
      />
      <span aria-live="polite">
        {page + 1} / {pages}
      </span>
      <Button
        icon="chevRight"
        style={iconButtonStyle}
        ariaLabel={t('appStyles.nextPage')}
        disabled={disabled || page + 1 >= pages}
        onClick={() => onChange(page + 1)}
      />
    </div>
  );
}

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
  const title = useId();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const [page, setPage] = useState(0);
  const selected = options.filter((app) => app.category === category).length;
  const visible = options.filter((app) =>
    app.name.toLocaleLowerCase().includes(search.toLocaleLowerCase()),
  );
  const pages = Math.max(1, Math.ceil(visible.length / 9));
  const currentPage = Math.min(page, pages - 1);
  return (
    <>
      <Button
        className="ol-app-category-button"
        style={{
          background: 'transparent',
          padding: '4px 0',
          justifyContent: 'flex-start',
          fontWeight: 600,
        }}
        disabled={busy}
        ariaLabel={t('appStyles.appsFor', { name: categoryName })}
        onClick={() => {
          setSearch('');
          setPage(0);
          setOpen(true);
        }}
      >
        <span>{categoryName}</span>
        <Icon name="chevRight" size={12} />
      </Button>
      {open && (
        <Dialog
          labelledBy={title}
          onClose={() => {
            if (!busy) setOpen(false);
          }}
          width="min(760px, 100%)"
          cardClassName="ol-app-members-dialog"
        >
          <DialogHeader>
            <h2 id={title}>{t('appStyles.chooseApps')}</h2>
            <div className="ol-app-members-heading">
              <span>{categoryName}</span>
              <Button
                icon="close"
                style={{ ...iconButtonStyle, borderRadius: '50%' }}
                ariaLabel={t('common.close')}
                disabled={busy}
                onClick={() => setOpen(false)}
              />
            </div>
          </DialogHeader>
          <div className="ol-app-members-search">
            <TextField
              aria-label={t('appStyles.searchApps')}
              placeholder={t('appStyles.searchApps')}
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
                setPage(0);
              }}
            />
          </div>
          <DialogBody>
            <div className="ol-app-members-grid" role="group" aria-label={categoryName}>
              {visible.slice(currentPage * 9, (currentPage + 1) * 9).map((app) => {
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
                    onClick={() => void onToggle(app)}
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
          </DialogBody>
          <DialogFooter>
            <span>{t('appStyles.selectedApps', { count: selected })}</span>
            <AppStylePagination
              page={currentPage}
              pages={pages}
              onChange={setPage}
              disabled={busy}
            />
          </DialogFooter>
        </Dialog>
      )}
    </>
  );
}
