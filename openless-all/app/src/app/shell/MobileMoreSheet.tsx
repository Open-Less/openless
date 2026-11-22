import { useTranslation } from 'react-i18next';
import {
  MobileNavigationItem,
  MobileNavigationSheet,
  type MobileTabSelect,
} from './MobileNavigationSheet';
import { MORE_TABS, type AppTab } from './navigation';

export function MobileMoreSheet({
  open,
  currentTab,
  onClose,
  onSelectTab,
  onOpenSettings,
}: {
  open: boolean;
  currentTab: AppTab;
  onClose: () => void;
  onSelectTab: MobileTabSelect;
  onOpenSettings: () => void;
}) {
  const { t } = useTranslation();
  return (
    <MobileNavigationSheet open={open} title={t('nav.more')} onClose={onClose}>
      {MORE_TABS.map(({ id, icon }) => (
        <MobileNavigationItem
          key={id}
          active={currentTab === id}
          icon={icon}
          label={t(`nav.${id}`)}
          onClick={() => {
            onSelectTab(id);
            onClose();
          }}
        />
      ))}
      <MobileNavigationItem
        icon="settings"
        label={t('shell.footer.settings')}
        onClick={() => {
          onOpenSettings();
          onClose();
        }}
      />
    </MobileNavigationSheet>
  );
}
