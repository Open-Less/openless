import { AnimatePresence, motion } from 'framer-motion';
import { useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { useReducedMotion } from '../../lib/motion';
import { ViewportPortal, useModalActive } from '../layout/AppViewport';
import type { InteractionController } from '../overlays/interactionController';

export function NotificationCenter({ controller }: { controller: InteractionController }) {
  const items = useSyncExternalStore(
    controller.subscribe,
    controller.getNotifications,
    controller.getNotifications,
  );
  const reduced = useReducedMotion();
  const modalActive = useModalActive();
  const { t } = useTranslation();

  return (
    <ViewportPortal>
      <div className="ol-notification-center" data-modal-active={modalActive || undefined}>
        <div className="ol-notification-stack">
          <AnimatePresence initial={false}>
            {items.map((item) => {
              const state = item.state ?? 'info';
              return (
                <motion.div
                  key={item.id}
                  className="ol-notification-card"
                  data-state={state}
                  initial={{ opacity: 0, y: reduced ? 0 : '-110%' }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: reduced ? 0 : '-110%' }}
                  transition={
                    reduced ? { duration: 0 } : { type: 'spring', stiffness: 320, damping: 30 }
                  }
                >
                  <span className="ol-notification-icon" aria-hidden="true">
                    {state === 'saving' ? (
                      <span className="ol-loading-spinner">
                        <Icon name="refresh" size={10} />
                      </span>
                    ) : state === 'deleted' ? (
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
                    <span role={state === 'failed' ? 'alert' : 'status'}>{item.message}</span>
                    {item.progress && <progress aria-label={item.message} />}
                  </span>
                  <button
                    disabled={modalActive}
                    type="button"
                    className="ol-notification-close"
                    aria-label={t('common.close')}
                    onClick={() => controller.dismiss(item.id)}
                  >
                    <Icon name="close" size={14} />
                  </button>
                </motion.div>
              );
            })}
          </AnimatePresence>
        </div>
      </div>
    </ViewportPortal>
  );
}
