import { AnimatePresence, motion, useIsPresent } from 'framer-motion';
import { useSyncExternalStore, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { useReducedMotion } from '../../lib/motion';
import { ViewportPortal, useModalActive } from '../layout/AppViewport';
import type { InteractionController, NotificationItem } from '../overlays/interactionController';

function NotificationCard({
  item,
  dismiss,
  modalActive,
}: {
  item: NotificationItem;
  dismiss: () => void;
  modalActive: boolean;
}) {
  const { t } = useTranslation();
  const reduced = useReducedMotion();
  const present = useIsPresent();
  const state = item.state ?? 'info';
  const iconState = state === 'deleting' ? 'deleted' : state;
  return (
    <motion.div
      className={`ol-notification-card${item.progress ? ' ol-notification-card--progress' : ''}`}
      data-state={state}
      aria-hidden={!present || undefined}
      initial={{ opacity: 0, y: reduced ? 0 : '-110%' }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: reduced ? 0 : '-110%' }}
      transition={reduced ? { duration: 0 } : { type: 'spring', stiffness: 320, damping: 30 }}
    >
      <span className={`ol-notification-icon ol-notification-icon--${iconState}`} aria-hidden="true">
        {state === 'saving' ? (
          <span className="ol-loading-spinner">
            <Icon name="refresh" size={10} />
          </span>
        ) : iconState === 'deleted' ? (
          <Icon name="trash" size={10} />
        ) : state === 'failed' ? (
          '!'
        ) : state === 'info' ? (
          'i'
        ) : (
          '✓'
        )}
      </span>
      <span className="ol-notification-body">
        <span className="ol-notification-message" role={state === 'failed' ? 'alert' : 'status'}>
          {item.message}
        </span>
        {item.progress && (
          <span className="ol-notification-progress" role="progressbar" aria-label={item.message}>
            <span className="ol-notification-progress-value" />
          </span>
        )}
      </span>
      {item.actionLabel && item.onAction && (
        <button
          type="button"
          className="ol-notification-action"
          disabled={modalActive || !present}
          onClick={() => {
            if (!item.progress) dismiss();
            item.onAction?.();
          }}
        >
          {item.actionLabel}
        </button>
      )}
      {!item.progress && (
        <button
          type="button"
          className="ol-notification-close"
          aria-label={t('common.close')}
          disabled={!present}
          tabIndex={modalActive ? -1 : undefined}
          onPointerDown={(event) => {
            // The notification can close without taking focus away from a dialog.
            if (modalActive) event.preventDefault();
          }}
          onClick={dismiss}
        >
          <Icon name="close" size={14} />
        </button>
      )}
    </motion.div>
  );
}

export function NotificationCenter({
  controller,
  aside,
}: {
  controller: InteractionController;
  aside?: ReactNode;
}) {
  const items = useSyncExternalStore(
    controller.subscribe,
    controller.getNotifications,
    controller.getNotifications,
  );
  const modalActive = useModalActive();
  return (
    <ViewportPortal>
      <div className="ol-notification-center">
        <div className="ol-notification-stack">
          <AnimatePresence initial={false}>
            {items.map((item) => (
              <NotificationCard
                key={item.id}
                item={item}
                dismiss={() => controller.dismiss(item.id)}
                modalActive={modalActive}
              />
            ))}
          </AnimatePresence>
          {aside}
        </div>
      </div>
    </ViewportPortal>
  );
}
