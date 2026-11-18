import type { ButtonHTMLAttributes, HTMLAttributes, ReactNode } from 'react';
import { Modal, type ModalProps } from './Modal';
import './dialog.css';

/** Structured dialogs share the viewport portal, stack, focus ownership and motion with Modal. */
export function Dialog({ children, ...props }: ModalProps) {
  return (
    <Modal
      {...props}
      cardClassName="ol-structured-dialog"
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
