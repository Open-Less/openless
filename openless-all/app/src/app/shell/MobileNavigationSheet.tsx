import { useId, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { ExitPresence, Modal } from '../../ui/overlays/Modal';
import type { AppTab } from './navigation';
import './navigation.css';

export function MobileNavigationSheet({
  open,
  title,
  onClose,
  children,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const titleId = useId();
  return (
    <ExitPresence
      open={open}
      render={(closing) => (
        <Modal
          closing={closing}
          labelledBy={titleId}
          motionVariant="sheet"
          onClose={onClose}
          overlayClassName="ol-mobile-navigation-sheet"
          width="100%"
          zIndex={60}
        >
          <div className="ol-mobile-navigation-sheet-header">
            <span id={titleId}>{title}</span>
            <button type="button" onClick={onClose} aria-label={t('common.close')}>
              <Icon name="close" size={16} />
            </button>
          </div>
          <div className="ol-mobile-navigation-sheet-items">{children}</div>
        </Modal>
      )}
    />
  );
}

export function MobileNavigationItem({
  active,
  icon,
  label,
  onClick,
}: {
  active?: boolean;
  icon: string;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? 'page' : undefined}
      className={['ol-nav-btn', active && 'ol-nav-btn-active'].filter(Boolean).join(' ')}
    >
      <Icon name={icon} size={16} />
      <span>{label}</span>
      <Icon name="chevRight" size={13} />
    </button>
  );
}

export type MobileTabSelect = (tab: AppTab) => void;
