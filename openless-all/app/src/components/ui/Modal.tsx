// Modal — centered dialog: backdrop + card. Marketplace details / upload / my
// listings / GitHub login all share this popup logic instead of each writing its own.
//
// Shared opacity/transform motion preserves the current pose when interrupted.

import { useEffect, useLayoutEffect, useRef, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useOverlayMotion } from '../../lib/motion';
import { useExitMount } from '../../lib/useExitMount';

interface ModalProps {
  children: ReactNode;
  onClose: () => void;
  /** Default 50; pass a larger value when stacking (e.g. login above "my listings"). */
  zIndex?: number;
  /** Card width, default 'min(560px, 100%)'. */
  width?: string;
  /** Dialogs with a fixed title/footer can let the inner content area scroll. */
  style?: CSSProperties;
  /** true plays the exit animation; the caller gates unmount timing with
   *  useExitMount, unmounting after the animation finishes. */
  closing?: boolean;
  /** Lets the host window customize overlay/card look (e.g. a rounded floating window needs a rounded overlay). */
  overlayClassName?: string;
  labelledBy?: string;
}

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), select:not([disabled]), input:not([disabled]):not([type="hidden"]), [tabindex]:not([tabindex="-1"])';

// Only the top-most dialog keeps Tab cycling inside itself.
const openModals: symbol[] = [];

function focusableWithin(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) => element.getClientRects().length > 0,
  );
}

export function Modal({
  children,
  onClose,
  zIndex = 50,
  width = 'min(560px, 100%)',
  style,
  closing = false,
  overlayClassName,
  labelledBy,
}: ModalProps) {
  const cardRef = useRef<HTMLDivElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  useOverlayMotion(overlayRef, closing, 'backdrop');
  useOverlayMotion(cardRef, closing);
  useLayoutEffect(() => {
    if (cardRef.current) cardRef.current.inert = closing;
  }, [closing]);

  useEffect(() => {
    const id = Symbol('modal');
    openModals.push(id);
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const card = cardRef.current;
    if (card && !card.contains(document.activeElement)) card.focus({ preventScroll: true });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Tab' || !card || openModals[openModals.length - 1] !== id) return;
      if (card.inert) {
        event.preventDefault();
        return;
      }
      const items = focusableWithin(card);
      const active = document.activeElement;
      if (items.length === 0) {
        event.preventDefault();
        card.focus({ preventScroll: true });
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey) {
        if (active === first || active === card || !card.contains(active)) {
          event.preventDefault();
          last.focus();
        }
      } else if (active === last || !card.contains(active)) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      const index = openModals.indexOf(id);
      if (index >= 0) openModals.splice(index, 1);
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, []);

  // Portal to document.body: dialogs often trigger from inside panels (settings /
  // marketplace), and the window chrome (WindowChrome) and page containers carry a
  // temporary transforms while animating, creating a containing block — rendered in
  // place, the backdrop's `position: fixed` would anchor to that ancestor instead of
  // the viewport, covering only the triggering panel (e.g. GitHub login floating over
  // a still-bright settings page). Portaled out, fixed anchors to the viewport and
  // the overlay covers the whole window. Same approach as Tooltip / SelectLite.
  return createPortal(
    <div
      ref={overlayRef}
      className={`ol-dialog-overlay${closing ? ' is-closing' : ''}${overlayClassName ? ` ${overlayClassName}` : ''}`}
      onClick={closing ? undefined : onClose}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'var(--ol-dialog-backdrop)',
        display: 'grid',
        placeItems: 'center',
        zIndex,
        padding: 20,
      }}
    >
      <div
        ref={cardRef}
        className="ol-dialog-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        style={{
          width,
          maxHeight: '85vh',
          overflow: 'auto',
          borderRadius: 'var(--ol-dialog-radius)',
          background: 'var(--ol-surface)',
          border: '1px solid var(--ol-dialog-border)',
          boxShadow: 'var(--ol-dialog-shadow)',
          padding: 24,
          outline: 'none',
          ...style,
        }}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

/** Preserve the last visible content while its non-interactive exit finishes. */
export function PresenceModal({
  open,
  render,
  ...props
}: Omit<ModalProps, 'children' | 'closing'> & { open: boolean; render: () => ReactNode }) {
  const presence = useExitMount(open);
  const previous = useRef<{ render: () => ReactNode; props: typeof props } | null>(null);
  const content = open ? { render, props } : previous.current;
  useLayoutEffect(() => {
    if (open) previous.current = { render, props };
    else if (!presence.mounted) previous.current = null;
  });
  if (!presence.mounted || !content) return null;
  return (
    <Modal {...content.props} closing={presence.closing}>
      {content.render()}
    </Modal>
  );
}
