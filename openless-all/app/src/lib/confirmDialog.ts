// One global confirmation surface for destructive actions. Pages await
// confirmDialog(...) instead of using window.confirm or their own modal, so every
// delete prompt shares the same size, focus trap and button treatment.
export interface ConfirmDialogOptions {
  title: string;
  description: string;
  confirmLabel?: string;
  cancelLabel?: string;
  tone?: 'danger' | 'default';
  icon?: string;
}

interface ConfirmRequest {
  id: number;
  options: ConfirmDialogOptions;
  resolve: (confirmed: boolean) => void;
}

let sequence = 0;
let current: ConfirmRequest | null = null;
const listeners = new Set<() => void>();
const publish = () => listeners.forEach((listener) => listener());

export function confirmDialog(options: ConfirmDialogOptions): Promise<boolean> {
  // A second request supersedes the first; the abandoned caller must not act.
  current?.resolve(false);
  return new Promise<boolean>((resolve) => {
    current = { id: ++sequence, options, resolve };
    publish();
  });
}

export function settleConfirm(confirmed: boolean) {
  const request = current;
  if (!request) return;
  current = null;
  publish();
  request.resolve(confirmed);
}

export function subscribeConfirm(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export const getConfirmRequest = () => current;
