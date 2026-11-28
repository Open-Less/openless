import type { NotificationOptions } from '../api/interaction';

interface NotificationSink {
  notify(options: NotificationOptions): void;
  dismissByKey(key: string): void;
}

let sink: NotificationSink | undefined;
let sequence = 0;

/** Non-React command producers publish through the mounted WebView's interaction controller. */
export function bindNotificationSink(next: NotificationSink): () => void {
  sink = next;
  return () => {
    if (sink === next) sink = undefined;
  };
}

const notificationKey = (id: string) => `legacy-notification:${id}`;

export function notify({
  id = `notification-${++sequence}`,
  ...item
}: NotificationOptions & { id?: string }): string {
  sink?.notify({ ...item, key: notificationKey(id) });
  return id;
}

export function dismissNotification(id: string): void {
  sink?.dismissByKey(notificationKey(id));
}
