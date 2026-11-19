import type { Interaction, NotificationOptions, ToastVariant } from '../../api/interaction';

type ToastOptions = Omit<NotificationOptions, 'message' | 'state' | 'severity' | 'variant'>;
type Message<T> = string | ((value: T) => string);

/** Presentation API over the window's existing notification lifetime and store. */
export function createToast(notify: Interaction['notify']) {
  const variant =
    (type: ToastVariant) =>
    (message: string, options: ToastOptions = {}) =>
      notify({ ...options, message, variant: type });
  const loading = variant('loading');
  return Object.assign(variant('default'), {
    success: variant('success'),
    error: variant('error'),
    warning: variant('warning'),
    info: variant('info'),
    loading,
    async promise<T>(
      task: Promise<T> | (() => Promise<T>),
      messages: { loading: string; success: Message<T>; error: Message<unknown> },
      options?: ToastOptions,
    ): Promise<T> {
      const handle = loading(messages.loading, { ...options, durationMs: 0 });
      try {
        const value = await (typeof task === 'function' ? task() : task);
        handle.update({
          variant: 'success',
          message:
            typeof messages.success === 'function' ? messages.success(value) : messages.success,
          durationMs: options?.durationMs,
          progress: false,
          actionLabel: undefined,
          onAction: undefined,
        });
        return value;
      } catch (error) {
        handle.update({
          variant: 'error',
          message: typeof messages.error === 'function' ? messages.error(error) : messages.error,
          durationMs: options?.durationMs,
          progress: false,
          actionLabel: undefined,
          onAction: undefined,
        });
        throw error;
      }
    },
  });
}

export function toastVariant(item: NotificationOptions): ToastVariant {
  if (item.variant) return item.variant;
  if (item.severity === 'warning') return 'warning';
  if (item.state === 'failed' || item.severity === 'error') return 'error';
  if (item.state === 'saving' || item.state === 'deleting' || item.progress) return 'loading';
  if (item.state === 'saved' || item.state === 'deleted') return 'success';
  return 'info';
}
