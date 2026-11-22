export type AndroidPanelKind = 'qa' | 'voice-edit';
export interface AndroidPanelView {
  kind: AndroidPanelKind | null;
  epoch: number;
  error?: string;
}

interface PanelTransition {
  target: AndroidPanelKind | null;
  promise: Promise<boolean>;
}

export function createAndroidPanelState(
  cancel: (kind: AndroidPanelKind, isCurrent: () => boolean) => Promise<void>,
  publish: (view: AndroidPanelView) => void,
) {
  let view: AndroidPanelView = { kind: null, epoch: 0 };
  let generation = 0;
  let pending: PanelTransition | undefined;
  let cancellation: { epoch: number; promise: Promise<void> } | undefined;
  let disposed = false;

  const show = (kind: AndroidPanelView['kind']) => {
    view = { kind, epoch: view.epoch + 1 };
    publish(view);
  };

  const switchTo = (target: AndroidPanelView['kind']): Promise<boolean> => {
    if (disposed) return Promise.resolve(false);
    if (pending?.target === target) return pending.promise;
    const current = view.kind;
    const request = ++generation;
    const isCurrent = () => !disposed && request === generation;
    if (!current || target === current) {
      // An explicit reopen wins over a pending close of the same surface.
      if (view.kind !== target || pending || view.error) show(target);
      pending = undefined;
      return Promise.resolve(true);
    }
    const source = view;
    if (cancellation?.epoch !== source.epoch) {
      const isSourceCurrent = () =>
        !disposed && view.epoch === source.epoch && view.kind === source.kind;
      const operation = {
        epoch: source.epoch,
        promise: Promise.resolve()
          .then(() => {
            if (isSourceCurrent()) return cancel(current, isSourceCurrent);
          })
          .finally(() => {
            if (cancellation === operation) cancellation = undefined;
          }),
      };
      cancellation = operation;
    }
    // Upgrading a close to a switch shares cleanup for the same panel generation.
    const promise = cancellation.promise
      .then(() => {
        if (!isCurrent()) return false;
        show(target);
        return true;
      })
      .catch((reason: unknown) => {
        if (isCurrent()) {
          view = { ...view, error: reason instanceof Error ? reason.message : String(reason) };
          publish(view);
        }
        return false;
      })
      .finally(() => {
        if (isCurrent()) pending = undefined;
      });
    pending = { target, promise };
    return promise;
  };

  const close = (expected: AndroidPanelView) =>
    expected.epoch === view.epoch && expected.kind === view.kind
      ? switchTo(null)
      : Promise.resolve(false);

  const nativeDismiss = (kind: AndroidPanelKind) => {
    // Native emits the event before IPC cleanup returns. Keep the visible surface
    // until the transition knows whether the operation actually succeeded.
    if (disposed || pending || view.kind !== kind) return;
    ++generation;
    show(null);
  };

  return {
    current: () => view,
    switchTo,
    close,
    nativeDismiss,
    generation: () => generation,
    dispose() {
      disposed = true;
      pending = undefined;
      cancellation = undefined;
      ++generation;
    },
  };
}
