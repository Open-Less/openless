import type { ButtonHTMLAttributes, HTMLAttributes, ReactNode } from 'react';
import { Modal, type ModalProps } from './Modal';
import './dialog.css';

export type DialogLayout = 'standard' | 'terms' | 'decision';

/** Structured dialogs share the viewport portal, stack, focus ownership and motion with Modal. */
export function Dialog({
  children,
  layout = 'standard',
  cardClassName,
  overlayClassName,
  width,
  ...props
}: ModalProps & { layout?: DialogLayout }) {
  const originLayout = layout !== 'standard';
  return (
    <Modal
      {...props}
      width={width ?? (layout === 'terms' ? 'min(512px, 100%)' : layout === 'decision' ? 'min(400px, 100%)' : undefined)}
      cardClassName={['ol-structured-dialog', `ol-structured-dialog--${layout}`, cardClassName].filter(Boolean).join(' ')}
      overlayClassName={[originLayout && 'ol-dialog-overlay--origin', overlayClassName].filter(Boolean).join(' ')}
      motionVariant={props.motionVariant ?? 'pop'}
    >
      {children}
    </Modal>
  );
}

export function DialogHeader({ children, ...props }: HTMLAttributes<HTMLElement>) {
  return (
    <header {...props} className={`ol-dialog-header ${props.className ?? ''}`}>
      {children}
    </header>
  );
}
export function DialogBody({ children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div {...props} className={`ol-dialog-body ${props.className ?? ''}`}>
      {children}
    </div>
  );
}
export function DialogFooter({ children, ...props }: HTMLAttributes<HTMLElement>) {
  return (
    <footer {...props} className={`ol-dialog-footer ${props.className ?? ''}`}>
      {children}
    </footer>
  );
}
export function DialogAction({
  tone = 'primary',
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  tone?: 'primary' | 'danger' | 'neutral';
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      {...props}
      className={`ol-pressable ol-action-button ol-dialog-action ol-dialog-action--${tone} ${props.className ?? ''}`}
    >
      {children}
    </button>
  );
}
