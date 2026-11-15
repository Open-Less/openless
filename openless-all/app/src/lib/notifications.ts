// One presentation contract for every API/page: saved uses the theme blue check,
// deleted uses the destructive red trash, failed uses the red alert and info/saving
// stay neutral. Pages choose a state; NotificationCenter owns the icon and colour.
export type NotificationState = 'saving' | 'saved' | 'deleted' | 'failed' | 'info';
export interface Notification {
  id: string;
  state: NotificationState;
  message: string;
  actionLabel?: string;
  onAction?: () => void;
  durationMs?: number;
  /** Renders an indeterminate progress bar under the message (long-running host work). */
  progress?: boolean;
}
let sequence = 0;
let notifications: Notification[] = [];
const listeners = new Set<() => void>();
const timers = new Map<string, number>();
const publish = () => listeners.forEach((listener) => listener());
const clearTimer = (id: string) => {
  const timer = timers.get(id);
  if (timer !== undefined) window.clearTimeout(timer);
  timers.delete(id);
};
const replace = (next: Notification[]) => {
  const keep = new Set(next.map((item) => item.id));
  for (const item of notifications) {
    if (!keep.has(item.id)) clearTimer(item.id);
  }
  notifications = next;
  publish();
};
const isTransientSuccess = (item: Notification) =>
  (item.state === 'saved' || item.state === 'deleted') && !item.actionLabel && !item.progress;
export function dismissNotification(id: string) {
  clearTimer(id);
  replace(notifications.filter((item) => item.id !== id));
}
export function notify(item: Omit<Notification, 'id'> & { id?: string }): string {
  const id = item.id ?? `notification-${++sequence}`;
  clearTimer(id);
  const next: Notification = { ...item, id };
  // A repeated auto-save should read as one toast dropping in again, not as an
  // ever-growing stack that pushes earlier confirmations down the window.
  const previous = isTransientSuccess(next)
    ? notifications.filter((entry) => !isTransientSuccess(entry) && entry.id !== id)
    : notifications.filter((entry) => entry.id !== id);
  replace([next, ...previous]);
  const duration = item.durationMs ?? (isTransientSuccess(next) ? 3500 : 0);
  if (duration > 0)
    timers.set(
      id,
      window.setTimeout(() => dismissNotification(id), duration),
    );
  return id;
}
export function subscribeNotifications(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
export const getNotifications = () => notifications;
