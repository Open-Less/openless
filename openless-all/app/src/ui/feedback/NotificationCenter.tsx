import { AnimatePresence, usePresence } from 'framer-motion';
import { useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { StatusNotice } from './StatusNotice';
import { ToastIcon, ToastPanel } from './ToastPanel';
import { toastVariant } from './toast';
import { ViewportPortal, useAppViewport } from '../layout/AppViewport';
import type { InteractionController, NotificationItem } from '../overlays/interactionController';

function NotificationCard({ item, dismiss }: { item: NotificationItem; dismiss: () => void }) {
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
  const below = item.actionStyle === 'below';
  const action = item.actionLabel && item.onAction && (
    <button
      type="button"
      data-slot="toast-action"
      className="ol-notification-action"
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
    </button>
  );

  return (
    <StatusNotice
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
          <div className="ol-notification-body">
            <div role={variant === 'error' || variant === 'warning' ? 'alert' : 'status'}>
              <span className="ol-notification-message" data-slot="toast-title">
                {item.message}
              </span>
              {item.description && (
                <span className="ol-notification-description" data-slot="toast-description">
                  {item.description}
                </span>
              )}
            </div>
            {item.progress && (
              <span
                className="ol-notification-progress"
                role="progressbar"
                aria-label={item.message}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={value}
              >
                <span
                  className="ol-notification-progress-value"
                  data-determinate={value !== undefined}
                  style={value === undefined ? undefined : { width: `${value}%` }}
                />
              </span>
            )}
          </div>
          {!below && action}
        </div>
        {below && action && <div className="ol-toast-footer">{action}</div>}
      </ToastPanel>
      <button
        type="button"
        className="ol-notification-close"
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
    </StatusNotice>
  );
}

export function NotificationCenter({ controller }: { controller: InteractionController }) {
  const items = useSyncExternalStore(
    controller.subscribe,
    controller.getNotifications,
    controller.getNotifications,
  );
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
              />
            ))}
          </AnimatePresence>
        </div>
      </div>
    </ViewportPortal>
  );
}
