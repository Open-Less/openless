import { AnimatePresence, motion } from 'framer-motion';
import { useSyncExternalStore, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import {
  dismissNotification,
  getNotifications,
  subscribeNotifications,
} from '../lib/notifications';
import { useReducedMotion } from '../lib/motion';
import { Icon } from './Icon';

export function NotificationCenter({ children }: { children?: ReactNode }) {
  const items = useSyncExternalStore(subscribeNotifications, getNotifications, getNotifications);
  const reduced = useReducedMotion();
  const { t } = useTranslation();
  // Portal to body: page/settings surfaces create their own stacking contexts, so a
  // fixed layer inside the app tree can still be painted below a portaled dialog.
  return createPortal(
    <div className="ol-notification-center">
      <AnimatePresence initial={false}>
        {items.map((item) => (
          <motion.div
            key={item.id}
            layout={!reduced}
            className={`ol-notification-card${item.progress ? ' ol-notification-card--progress' : ''}`}
            initial={{ opacity: 0, y: reduced ? 0 : '-110%' }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: reduced ? 0 : '-110%' }}
            transition={reduced ? { duration: 0 } : { type: 'spring', stiffness: 320, damping: 30 }}
          >
            <span
              className={`ol-notification-icon ol-notification-icon--${item.state}`}
              aria-hidden="true"
            >
              {item.state === 'saving' ? (
                <span className="ol-loading-spinner">
                  <Icon name="refresh" size={10} />
                </span>
              ) : item.state === 'failed' ? (
                '!'
              ) : item.state === 'info' ? (
                'i'
              ) : item.state === 'deleted' ? (
                <Icon name="trash" size={10} />
              ) : (
                '✓'
              )}
            </span>
            <span className="ol-notification-body">
              <span
                className="ol-notification-message"
                role={item.state === 'failed' ? 'alert' : 'status'}
              >
                {item.message}
              </span>
              {item.progress && (
                <span
                  className="ol-notification-progress"
                  role="progressbar"
                  aria-valuetext={item.message}
                >
                  <span className="ol-notification-progress-value" />
                </span>
              )}
            </span>
            {item.actionLabel && item.onAction && (
              <button
                type="button"
                className="ol-notification-action"
                onClick={() => {
                  item.onAction?.();
                  dismissNotification(item.id);
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
                onClick={() => dismissNotification(item.id)}
              >
                <Icon name="close" size={14} />
              </button>
            )}
          </motion.div>
        ))}
      </AnimatePresence>
      {children}
    </div>,
    document.body,
  );
}
