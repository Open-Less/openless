import {
  AnimatePresence,
  motion,
  useIsPresent,
  usePresence,
  type HTMLMotionProps,
} from 'framer-motion';
import { forwardRef, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { useReducedMotion } from '../../lib/motion';
import { ToastNotice } from './ToastNotice';
import { ToastIcon, ToastPanel } from './ToastPanel';
import { toastVariant } from './toast';
import { ViewportPortal, useAppViewport } from '../layout/AppViewport';
import type { InteractionController, NotificationItem } from '../overlays/interactionController';

function NotificationAction({ disabled, onClick, ...props }: HTMLMotionProps<'button'>) {
  const present = useIsPresent();
  const reduced = useReducedMotion();
  return (
    <motion.button
      {...props}
      disabled={!present || disabled}
      data-action-disabled={disabled || undefined}
      aria-hidden={!present || undefined}
      exit={{ opacity: 0, x: reduced ? 0 : 4 }}
      transition={{ duration: reduced ? 0 : 0.12, ease: 'easeOut' }}
      onClick={present ? onClick : undefined}
    />
  );
}

const NotificationCard = forwardRef<
  HTMLDivElement,
  { item: NotificationItem; dismiss: () => void }
>(function NotificationCard({ item, dismiss }, ref) {
  const { t } = useTranslation();
  const [present, remove] = usePresence();
  const { modalStack } = useAppViewport();
  const top = useSyncExternalStore(modalStack.subscribe, modalStack.getTopOverlay);
  const variant = toastVariant(item);
  const state =
    item.state ?? (variant === 'loading' ? 'saving' : variant === 'error' ? 'failed' : 'info');
  const percent = item.progressValue;
  const value =
    percent != null && Number.isFinite(percent) ? Math.min(100, Math.max(0, percent)) : undefined;
  const actionAllowed = (overlay: HTMLElement | null) => {
    const anchor = item.actionOwner?.();
    return !overlay || Boolean(anchor && overlay.contains(anchor));
  };
  const usesIcon = item.actionStyle === 'cancel-icon';
  const action = item.actionLabel && item.onAction && (
    <NotificationAction
      key="action"
      type="button"
      data-slot="toast-action"
      className="ol-toast-notification-action"
      data-action-style={item.actionStyle}
      aria-label={item.actionLabel}
      title={item.actionTitle ?? item.actionLabel}
      disabled={!present || item.actionDisabled || !actionAllowed(top)}
      tabIndex={top ? -1 : undefined}
      onPointerDown={(event) => {
        if (top) event.preventDefault();
      }}
      onClick={() => {
        if (!present || !actionAllowed(modalStack.getTopOverlay())) return;
        if (variant !== 'loading' && !item.progress) dismiss();
        item.onAction?.();
      }}
    >
      {usesIcon ? <Icon name="close" size={16} /> : item.actionLabel}
    </NotificationAction>
  );

  return (
    <div ref={ref} className="ol-toast-notification-slot" data-exiting={!present || undefined}>
      <ToastNotice
        state={state}
        variant={variant}
        notificationKey={item.key}
        progress={item.progress}
        severity={item.severity}
        present={present}
        onExit={remove ?? undefined}
      >
        <ToastPanel>
          <div className="ol-toast-row">
            {variant !== 'default' && <ToastIcon type={variant} />}
            <div className="ol-toast-notification-body">
              {item.progress && (
                <span
                  className="ol-toast-notification-progress"
                  role="progressbar"
                  aria-label={item.message}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={value}
                >
                  <span
                    className="ol-toast-notification-progress-value"
                    data-determinate={value !== undefined}
                    style={value === undefined ? undefined : { width: `${value}%` }}
                  />
                </span>
              )}
              <div role={variant === 'error' || variant === 'warning' ? 'alert' : 'status'}>
                <span className="ol-toast-notification-message" data-slot="toast-title">
                  {item.message}
                </span>
                {item.description && (
                  <span className="ol-toast-notification-description" data-slot="toast-description">
                    {item.description}
                  </span>
                )}
              </div>
            </div>
            <AnimatePresence initial={false}>{action}</AnimatePresence>
          </div>
        </ToastPanel>
        <button
          type="button"
          className="ol-toast-notification-close"
          data-slot="toast-close"
          aria-label={t('common.close')}
          title={t('common.close')}
          disabled={!present}
          tabIndex={top ? -1 : undefined}
          onPointerDown={(event) => {
            if (top) event.preventDefault();
          }}
          onClick={dismiss}
        >
          <Icon name="close" size={10} />
        </button>
      </ToastNotice>
    </div>
  );
});

export function ToastNotificationCenter({ controller }: { controller: InteractionController }) {
  const items = useSyncExternalStore(
    controller.subscribe,
    controller.getNotifications,
    controller.getNotifications,
  );
  return (
    <ViewportPortal>
      <div className="ol-toast-notification-center">
        <div className="ol-toast-notification-stack">
          <AnimatePresence initial={false} mode="popLayout">
            {items.map((item) => (
              <NotificationCard
                key={item.id}
                item={item}
                dismiss={() => controller.dismiss(item.id)}
              />
            ))}
          </AnimatePresence>
        </div>
      </div>
    </ViewportPortal>
  );
}
