import type { ReactNode } from 'react';
import type {
  ConfirmOptions,
  DialogOptions,
  Interaction,
  NotificationHandle,
  NotificationOptions,
  WarningOptions,
} from '../../api/interaction';

type Owner = symbol;
type Request = { id: number; owner: Owner; cancel: () => void; release: () => void } & (
  | { kind: 'confirm'; options: ConfirmOptions; accept: () => void }
  | { kind: 'warning'; options: WarningOptions; accept: () => void }
  | { kind: 'dialog'; options: Omit<DialogOptions<unknown>, 'render'>; render: () => ReactNode }
);

export type NotificationItem = NotificationOptions & { id: number; owner: Owner };

const dismissed: NotificationHandle = { dismiss: () => {}, update: () => {} };

export function createInteractionController() {
  let sequence = 0;
  let request: Request | null = null;
  let notifications: readonly NotificationItem[] = [];
  const timers = new Map<number, ReturnType<typeof setTimeout>>();
  const listeners = new Set<() => void>();
  const publish = () => listeners.forEach((listener) => listener());

  function complete(id: number, onComplete: () => void) {
    if (request?.id !== id) return;
    request = null;
    onComplete();
    publish();
  }

  function replace(next: Request) {
    const previous = request;
    request = next;
    previous?.release();
    publish();
  }

  function notifyItems(next: readonly NotificationItem[]) {
    notifications = next;
    publish();
  }

  function clearTimer(id: number) {
    const timer = timers.get(id);
    if (timer !== undefined) clearTimeout(timer);
    timers.delete(id);
  }

  function dismiss(id: number) {
    clearTimer(id);
    if (notifications.some((item) => item.id === id))
      notifyItems(notifications.filter((item) => item.id !== id));
  }

  function startTimer(item: NotificationItem) {
    clearTimer(item.id);
    const duration = item.durationMs ?? (isTransientSuccess(item) ? 3500 : 0);
    if (duration > 0) {
      timers.set(
        item.id,
        setTimeout(() => {
          if (notifications.includes(item)) dismiss(item.id);
        }, duration),
      );
    }
  }

  function isTransientSuccess(item: NotificationOptions) {
    return (
      !item.progress &&
      !item.actionLabel &&
      !item.severity &&
      (item.state === 'saved' || item.state === 'deleted')
    );
  }

  function pruneFor(item: NotificationItem, keepId = false) {
    return notifications.filter((entry) => {
      if (keepId && entry.id === item.id) return true;
      const replaced =
        (item.key !== undefined && entry.key === item.key) ||
        (isTransientSuccess(item) && isTransientSuccess(entry));
      if (replaced) clearTimer(entry.id);
      return !replaced;
    });
  }

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getRequest: () => request,
    getNotifications: () => notifications,
    dismiss,
    dismissByKey(key: string) {
      const item = notifications.find((entry) => entry.key === key);
      if (item) dismiss(item.id);
    },
    clear() {
      request?.cancel();
      if (notifications.length === 0) return;
      for (const item of notifications) clearTimer(item.id);
      notifyItems([]);
    },
    createScope(): { interaction: Interaction; activate: () => void; dispose: () => void } {
      const owner = Symbol('interaction scope');
      let active = true;
      const confirm: Interaction['confirm'] = (options) => {
        if (!active) return Promise.resolve(false);
        return new Promise((resolve) => {
          const id = ++sequence;
          replace({
            id,
            owner,
            kind: 'confirm',
            options,
            release: () => resolve(false),
            cancel: () => complete(id, () => resolve(false)),
            accept: () => complete(id, () => resolve(true)),
          });
        });
      };
      const warn: Interaction['warn'] = (options) => {
        if (!active) return Promise.resolve(false);
        return new Promise((resolve) => {
          const id = ++sequence;
          replace({
            id,
            owner,
            kind: 'warning',
            options,
            release: () => resolve(false),
            cancel: () => complete(id, () => resolve(false)),
            accept: () => complete(id, () => resolve(true)),
          });
        });
      };
      const openDialog: Interaction['openDialog'] = <T>(options: DialogOptions<T>) => {
        if (!active) return Promise.resolve(undefined);
        return new Promise<T | undefined>((resolve) => {
          const id = ++sequence;
          const cancel = () => complete(id, () => resolve(undefined));
          replace({
            id,
            owner,
            kind: 'dialog',
            cancel,
            release: () => resolve(undefined),
            options,
            render: () =>
              options.render({
                resolve: (value) => complete(id, () => resolve(value)),
                cancel,
              }),
          });
        });
      };

      return {
        interaction: {
          confirm,
          warn,
          openDialog,
          notify(options) {
            if (!active) return dismissed;
            const existing =
              options.key === undefined
                ? undefined
                : notifications.find((entry) => entry.key === options.key && entry.owner === owner);
            const id = existing?.id ?? ++sequence;
            const item: NotificationItem = { ...options, id, owner };
            notifyItems(
              existing
                ? pruneFor(item, true).map((entry) => (entry.id === id ? item : entry))
                : [item, ...pruneFor(item)],
            );
            startTimer(item);
            return {
              dismiss: () => dismiss(id),
              update(change) {
                const current = notifications.find((entry) => entry.id === id);
                if (!current) return;
                const updated = { ...current, ...change };
                notifyItems(
                  pruneFor(updated, true).map((entry) => (entry.id === id ? updated : entry)),
                );
                startTimer(updated);
              },
            };
          },
        },
        activate: () => {
          active = true;
        },
        dispose() {
          active = false;
          if (request?.owner === owner) request.cancel();
          const kept = notifications.filter((item) => {
            if (item.owner === owner) clearTimer(item.id);
            return item.owner !== owner;
          });
          if (kept.length !== notifications.length) notifyItems(kept);
        },
      };
    },
  };
}

export type InteractionController = ReturnType<typeof createInteractionController>;
