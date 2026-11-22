import type { ReactNode } from 'react';

export interface ConfirmOptions {
  title: ReactNode;
  description?: ReactNode;
  tone?: 'default' | 'danger';
  icon?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** Keep the confirmation open until the operation succeeds; rejection permits retry. */
  onConfirm?: () => void | Promise<void>;
  /** Supply a safe, localized domain error; raw exception messages are never rendered. */
  errorMessage?: ReactNode | ((error: unknown) => ReactNode);
}

export interface WarningOptions {
  title: ReactNode;
  description?: ReactNode;
  dismissLabel?: string;
}

export type NotificationState = 'info' | 'saved' | 'failed' | 'deleting' | 'deleted' | 'saving';

export interface NotificationOptions {
  message: string;
  description?: string;
  state?: NotificationState;
  severity?: 'warning' | 'error';
  durationMs?: number;
  key?: string;
  progress?: boolean;
  actionLabel?: string;
  onAction?: () => void;
}

export interface NotificationHandle {
  dismiss: () => void;
  update: (change: Partial<Omit<NotificationOptions, 'key'>>) => void;
}

export interface DialogActions<T> {
  resolve: (value: T) => void;
  cancel: () => void;
}

export interface DialogOptions<T> {
  title: ReactNode;
  description?: ReactNode;
  width?: string;
  render: (actions: DialogActions<T>) => ReactNode;
}

export interface Interaction {
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  warn: (options: WarningOptions) => Promise<boolean>;
  notify: (options: NotificationOptions) => NotificationHandle;
  openDialog: <T>(options: DialogOptions<T>) => Promise<T | undefined>;
}
