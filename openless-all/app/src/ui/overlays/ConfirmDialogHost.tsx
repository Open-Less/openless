import { useId, useRef, useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { Dialog, DialogAction, DialogBody, DialogFooter, DialogHeader } from './Dialog';
import { ExitPresence } from './Modal';
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
  const pending = useRef<number | null>(null);
  const [operation, setOperation] = useState<{ id: number; busy: boolean; error?: unknown } | null>(
    null,
  );
  const busy = request != null && operation?.id === request.id && operation.busy;
  const cancel = () => {
    if (request && pending.current !== request.id) request.cancel();
  };
  const accept = async () => {
    if (!request || request.kind === 'dialog' || pending.current === request.id) return;
    if (request.kind !== 'confirm' || !request.options.onConfirm) {
      request.accept();
      return;
    }
    const active = request;
    pending.current = active.id;
    setOperation({ id: active.id, busy: true });
    try {
      await active.options.onConfirm!();
      // A replaced/disposed request resolves false through its controller owner.
      if (controller.getRequest()?.id === active.id) active.accept();
    } catch (error) {
      if (controller.getRequest()?.id === active.id)
        setOperation({ id: active.id, busy: false, error });
    } finally {
      if (pending.current === active.id) pending.current = null;
    }
  };
  return (
    <ExitPresence
      open={!!request}
      render={(closing) => {
        if (!request) return null;
        const custom = request.kind === 'dialog';
        const danger = request.kind === 'confirm' && request.options.tone === 'danger';
        const warning = request.kind === 'warning';
        const failed = operation?.id === request.id && !operation.busy && 'error' in operation;
        const error =
          request.kind === 'confirm' && failed
            ? typeof request.options.errorMessage === 'function'
              ? request.options.errorMessage(operation?.error)
              : (request.options.errorMessage ?? t('common.error'))
            : null;
        return (
          <Dialog
            closing={closing}
            onClose={cancel}
            zIndex={REQUEST_LAYER}
            focusKey={request.id}
            layout={danger ? 'decision' : 'standard'}
            width={custom ? request.options.width : danger ? undefined : 'min(480px, 100%)'}
            labelledBy={titleId}
            describedBy={request.options.description != null ? descriptionId : undefined}
            initialFocus={leastDestructive}
          >
            {custom ? (
              <>
                <DialogHeader>
                  <h2 id={titleId}>{request.options.title}</h2>
                </DialogHeader>
                <DialogBody>
                  {request.options.description != null && (
                    <div id={descriptionId} className="ol-confirm-dialog-description">
                      {request.options.description}
                    </div>
                  )}
                  {request.render()}
                </DialogBody>
              </>
            ) : (
              <div
                className="ol-confirm-dialog ol-dialog-flow"
                data-tone={warning ? 'warning' : danger ? 'danger' : 'default'}
                aria-busy={busy}
              >
                <DialogHeader>
                  <div>
                    <span className="ol-confirm-dialog-icon" aria-hidden="true">
                      <Icon
                        name={
                          request.kind === 'confirm'
                            ? (request.options.icon ?? (danger ? 'alert' : 'info'))
                            : 'info'
                        }
                        size={18}
                      />
                    </span>
                    <h2 id={titleId} className="ol-confirm-dialog-title">
                      {request.options.title}
                    </h2>
                  </div>
                  {danger && (
                    <button
                      type="button"
                      className="ol-dialog-icon-close"
                      aria-label={t('common.close')}
                      disabled={busy}
                      onClick={cancel}
                    >
                      <Icon name="close" size={16} />
                    </button>
                  )}
                </DialogHeader>
                <DialogBody>
                  {request.options.description != null && (
                    <div id={descriptionId} className="ol-confirm-dialog-description">
                      {request.options.description}
                    </div>
                  )}
                  {error != null && (
                    <p role="alert" className="ol-confirm-dialog-error">
                      {error}
                    </p>
                  )}
                </DialogBody>
                <DialogFooter>
                  {request.kind === 'warning' ? (
                    <button
                      ref={leastDestructive}
                      type="button"
                      className="ol-pressable ol-action-button ol-dialog-action ol-dialog-action--primary"
                      onClick={() => void accept()}
                    >
                      {request.options.dismissLabel ?? t('common.close')}
                    </button>
                  ) : (
                    <>
                      <button
                        ref={leastDestructive}
                        type="button"
                        className="ol-pressable ol-action-button ol-dialog-action ol-dialog-action--neutral ol-confirm-cancel"
                        disabled={busy}
                        onClick={cancel}
                      >
                        {request.options.cancelLabel ?? t('common.cancel')}
                      </button>
                      <DialogAction
                        tone={danger ? 'danger' : 'primary'}
                        className={danger ? 'ol-confirm-danger' : 'ol-confirm-primary'}
                        disabled={busy}
                        onClick={() => void accept()}
                      >
                        {request.options.confirmLabel ?? t('common.confirmAction')}
                      </DialogAction>
                    </>
                  )}
                </DialogFooter>
              </div>
            )}
          </Dialog>
        );
      }}
    />
  );
}
