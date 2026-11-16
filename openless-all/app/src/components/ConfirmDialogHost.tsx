import { useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from './ui/Modal';
import { Icon } from './Icon';
import { getConfirmRequest, settleConfirm, subscribeConfirm } from '../lib/confirmDialog';

/**
 * The only confirmation popup in the app. Pages await `confirmDialog(...)`; this host
 * renders it above every dialog (channel editors, settings, marketplace) with one
 * consistent 480px card, focus trap, Escape handling and button order.
 */
export function ConfirmDialogHost() {
  const request = useSyncExternalStore(subscribeConfirm, getConfirmRequest, getConfirmRequest);
  const { t } = useTranslation();
  if (!request) return null;
  const options = request.options;
  const danger = options.tone !== 'default';
  const cancel = () => settleConfirm(false);
  return (
    <Modal
      onClose={cancel}
      zIndex={1200}
      motionVariant="pop"
      labelledBy="ol-confirm-dialog-title"
      width="min(480px, calc(100vw - 40px))"
      style={{ padding: 30 }}
    >
      <div className="ol-confirm-dialog" data-tone={danger ? 'danger' : 'default'}>
        <span className="ol-confirm-dialog-icon" aria-hidden="true">
          <Icon name={options.icon ?? (danger ? 'trash' : 'info')} size={18} />
        </span>
        <h2 id="ol-confirm-dialog-title" className="ol-confirm-dialog-title">
          {options.title}
        </h2>
        <p className="ol-confirm-dialog-description">{options.description}</p>
        <div className="ol-confirm-dialog-actions">
          <button
            type="button"
            className={
              danger
                ? 'ol-pressable ol-action-button ol-confirm-danger'
                : 'ol-pressable ol-action-button ol-confirm-primary'
            }
            onClick={() => settleConfirm(true)}
          >
            {options.confirmLabel ?? t('common.confirmAction')}
          </button>
          <button
            type="button"
            autoFocus
            className="ol-pressable ol-action-button ol-confirm-cancel"
            onClick={cancel}
          >
            {options.cancelLabel ?? t('common.cancel')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
