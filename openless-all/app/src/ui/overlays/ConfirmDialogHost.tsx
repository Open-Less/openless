import { useId, useRef, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { PresenceModal } from './Modal';
import { REQUEST_LAYER } from './modalStack';
import type { InteractionController } from './interactionController';

export function ConfirmDialogHost({ controller }: { controller: InteractionController }) {
  const request = useSyncExternalStore(
    controller.subscribe,
    controller.getRequest,
    controller.getRequest,
  );
  const { t } = useTranslation();
  const titleId = useId();
  const descriptionId = useId();
  const leastDestructive = useRef<HTMLButtonElement>(null);
  return (
    <PresenceModal
      open={!!request}
      render={() => {
        if (!request) return null;
        if (request.kind === 'dialog') {
          return (
            <div key={request.id} className="ol-interaction-dialog">
              <h2 id={titleId} className="ol-confirm-dialog-title">
                {request.options.title}
              </h2>
              {request.options.description != null && (
                <div id={descriptionId} className="ol-confirm-dialog-description">
                  {request.options.description}
                </div>
              )}
              {request.render()}
            </div>
          );
        }
        const danger = request.kind === 'confirm' && request.options.tone === 'danger';
        const warning = request.kind === 'warning';
        const icon = request.kind === 'confirm' ? request.options.icon : undefined;
        return (
          <div
            className="ol-confirm-dialog"
            data-tone={warning ? 'warning' : danger ? 'danger' : 'default'}
          >
            <span className="ol-confirm-dialog-icon" aria-hidden="true">
              <Icon name={icon ?? (danger ? 'trash' : 'info')} size={18} />
            </span>
            <h2 id={titleId} className="ol-confirm-dialog-title">
              {request.options.title}
            </h2>
            {request.options.description != null && (
              <div id={descriptionId} className="ol-confirm-dialog-description">
                {request.options.description}
              </div>
            )}
            <div className="ol-confirm-dialog-actions">
              {request.kind === 'warning' ? (
                <button
                  ref={leastDestructive}
                  type="button"
                  className="ol-pressable ol-action-button ol-confirm-primary"
                  onClick={request.accept}
                >
                  {request.options.dismissLabel ?? t('common.close')}
                </button>
              ) : (
                <>
                  <button
                    type="button"
                    className={`ol-pressable ol-action-button ${danger ? 'ol-confirm-danger' : 'ol-confirm-primary'}`}
                    onClick={request.accept}
                  >
                    {request.options.confirmLabel ?? t('common.confirmAction')}
                  </button>
                  <button
                    ref={leastDestructive}
                    type="button"
                    className="ol-pressable ol-action-button ol-confirm-cancel"
                    onClick={request.cancel}
                  >
                    {request.options.cancelLabel ?? t('common.cancel')}
                  </button>
                </>
              )}
            </div>
          </div>
        );
      }}
      onClose={() => request?.cancel()}
      zIndex={REQUEST_LAYER}
      focusKey={request?.id}
      width={
        request?.kind === 'dialog'
          ? (request.options.width ?? 'min(560px, 100%)')
          : 'min(480px, 100%)'
      }
      motionVariant="pop"
      style={request?.kind === 'dialog' ? undefined : { padding: 'clamp(16px, 5vmin, 30px)' }}
      labelledBy={titleId}
      describedBy={request?.options.description != null ? descriptionId : undefined}
      initialFocus={leastDestructive}
    />
  );
}
