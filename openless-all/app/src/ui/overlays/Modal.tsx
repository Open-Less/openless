import {
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from 'react';
import { useOverlayMotion } from '../../lib/motion';
import { useExitMount } from '../../lib/useExitMount';
import { PopupContainer, ViewportPortal, useAppViewport } from '../layout/AppViewport';
import { DIALOG_LAYER, OPEN_POPUP, type ModalLayer } from './modalStack';

export interface ModalProps {
  children: ReactNode;
  onClose: () => void;
  zIndex?: number;
  width?: string;
  style?: CSSProperties;
  closing?: boolean;
  overlayClassName?: string;
  cardClassName?: string;
  labelledBy?: string;
  describedBy?: string;
  initialFocus?: RefObject<HTMLElement>;
  focusKey?: number | string;
  motionVariant?: 'card' | 'pop' | 'sheet';
}

export function Modal({
  children,
  onClose,
  zIndex = DIALOG_LAYER,
  width = 'min(560px, 100%)',
  style,
  closing = false,
  overlayClassName,
  cardClassName,
  labelledBy,
  describedBy,
  initialFocus,
  focusKey,
  motionVariant = 'card',
}: ModalProps) {
  const { boundary, modalStack } = useAppViewport();
  const cardRef = useRef<HTMLDivElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const [popupHost, setPopupHost] = useState<HTMLDivElement | null>(null);
  const config = useRef({ onClose, initialFocus, closing, zIndex });
  config.current = { onClose, initialFocus, closing, zIndex };
  const returnFocus = useRef(modalStack.getReturnFocus());
  const pressedWithPopup = useRef(false);
  const registered = useRef<ModalLayer | null>(null);

  useOverlayMotion(overlayRef, closing, 'backdrop');
  useOverlayMotion(cardRef, closing, motionVariant);

  useLayoutEffect(() => {
    if (!overlayRef.current || !cardRef.current) return;
    const layer: ModalLayer = {
      overlay: overlayRef.current,
      card: cardRef.current,
      close: () => config.current.onClose(),
      focus: () => config.current.initialFocus?.current ?? null,
      returnFocus: returnFocus.current,
      priority: config.current.zIndex,
      closing: config.current.closing,
    };
    registered.current = layer;
    const unregister = modalStack.register(layer);
    return () => {
      registered.current = null;
      unregister();
    };
  }, [modalStack]);

  useLayoutEffect(() => {
    if (!registered.current) return;
    if (registered.current.closing && !closing) {
      registered.current.returnFocus = modalStack.getReturnFocus();
    }
    registered.current.closing = closing;
    registered.current.priority = zIndex;
    modalStack.refresh();
  }, [closing, zIndex, modalStack]);

  useLayoutEffect(() => {
    if (focusKey !== undefined && registered.current) modalStack.focus(registered.current);
  }, [focusKey, modalStack]);

  return (
    <ViewportPortal>
      <div
        ref={overlayRef}
        className={['ol-dialog-overlay', closing && 'is-closing', overlayClassName]
          .filter(Boolean)
          .join(' ')}
        data-ol-contained={boundary ? '' : undefined}
        onPointerDownCapture={(event) => {
          pressedWithPopup.current =
            event.target === event.currentTarget && !!popupHost?.querySelector(OPEN_POPUP);
        }}
        onClick={(event) => {
          if (event.target !== event.currentTarget) return;
          event.stopPropagation();
          const layer = registered.current;
          const dismiss = !pressedWithPopup.current && !popupHost?.querySelector(OPEN_POPUP);
          pressedWithPopup.current = false;
          if (dismiss && layer && modalStack.isTop(layer)) onClose();
        }}
        style={{ zIndex }}
      >
        <PopupContainer.Provider value={popupHost}>
          <div
            ref={cardRef}
            className={['ol-dialog-card', cardClassName].filter(Boolean).join(' ')}
            role="dialog"
            aria-labelledby={labelledBy}
            aria-describedby={describedBy}
            tabIndex={-1}
            onClick={(event) => event.stopPropagation()}
            style={{ width, ...style }}
          >
            {children}
          </div>
        </PopupContainer.Provider>
        <div className="ol-dialog-popups" ref={setPopupHost} />
      </div>
    </ViewportPortal>
  );
}

/** Retain the last render closure (including its data) until exit completes. */
export function ExitPresence({
  open,
  render,
}: {
  open: boolean;
  render: (closing: boolean) => ReactNode;
}) {
  const presence = useExitMount(open);
  const previous = useRef<typeof render | null>(null);
  const content = open ? render : previous.current;
  useLayoutEffect(() => {
    if (open) previous.current = render;
    else if (!presence.mounted) previous.current = null;
  });
  if (!presence.mounted || !content) return null;
  return <>{content(presence.closing)}</>;
}

/** Preserve the last visible content while its non-interactive exit finishes. */
export function PresenceModal({
  open,
  render,
  ...props
}: Omit<ModalProps, 'children' | 'closing'> & { open: boolean; render: () => ReactNode }) {
  return (
    <ExitPresence
      open={open}
      render={(closing) => (
        <Modal {...props} closing={closing}>
          {render()}
        </Modal>
      )}
    />
  );
}
