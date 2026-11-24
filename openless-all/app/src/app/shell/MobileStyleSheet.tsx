import { useTranslation } from 'react-i18next';
import {
  MobileNavigationItem,
  MobileNavigationSheet,
  type MobileTabSelect,
} from './MobileNavigationSheet';
import { STYLE_TABS, subItemLabelKey, type AppTab } from './navigation';

export function MobileStyleSheet({
  open,
  currentTab,
  onClose,
  onSelectTab,
}: {
  open: boolean;
  currentTab: AppTab;
  onClose: () => void;
  onSelectTab: MobileTabSelect;
}) {
  const { t } = useTranslation();
  return (
    <MobileNavigationSheet open={open} title={t('nav.group.style')} onClose={onClose}>
      {STYLE_TABS.map(({ id, icon }) => (
        <MobileNavigationItem
          key={id}
          active={currentTab === id}
          icon={icon}
          label={t(subItemLabelKey(id))}
          onClick={() => {
            onSelectTab(id);
            onClose();
          }}
        />
      ))}
    </MobileNavigationSheet>
  );
}
