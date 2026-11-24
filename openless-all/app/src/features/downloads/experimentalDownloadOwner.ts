import { useMemo, useSyncExternalStore } from 'react';
import { EXPERIMENTAL_ASR_MODELS, type ExperimentalAsrModel } from '../../lib/experimentalAsr';
import { useAppViewport } from '../../ui/layout/AppViewport';

type Viewport = ReturnType<typeof useAppViewport>['modalStack'];
const inactiveAttributes = ['hidden', 'inert', 'aria-hidden'];
const interactive = (anchor: HTMLElement) =>
  anchor.isConnected && !anchor.closest('[hidden], [inert], [aria-hidden="true"]');

function createOwners() {
  const anchors = new Map<ExperimentalAsrModel, HTMLElement>();
  const listeners = new Set<() => void>();
  let revision = 0;
  const publish = () => {
    revision++;
    for (const listener of listeners) listener();
  };
  return {
    getSnapshot: () => revision,
    find(model: ExperimentalAsrModel) {
      const anchor = anchors.get(model);
      return anchor && interactive(anchor) ? anchor : null;
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    register(model: ExperimentalAsrModel, anchor: HTMLElement) {
      anchors.set(model, anchor);
      // Settings keeps incoming and outgoing pages mounted but inert during navigation.
      let available = interactive(anchor);
      const observer = new MutationObserver(() => {
        const next = interactive(anchor);
        if (available === next) return;
        available = next;
        publish();
      });
      for (let element: HTMLElement | null = anchor; element; element = element.parentElement) {
        observer.observe(element, { attributeFilter: inactiveAttributes });
      }
      publish();
      return () => {
        observer.disconnect();
        if (anchors.get(model) !== anchor) return;
        anchors.delete(model);
        publish();
      };
    },
  };
}
const owners = new WeakMap<Viewport, ReturnType<typeof createOwners>>();
function useOwners() {
  const { modalStack } = useAppViewport();
  let store = owners.get(modalStack);
  if (!store) {
    store = createOwners();
    owners.set(modalStack, store);
  }
  return store;
}

export function useExperimentalDownloadOwnerRefs() {
  const store = useOwners();
  return useMemo(() => {
    const refs = {} as Record<ExperimentalAsrModel, (anchor: HTMLElement | null) => void>;
    for (const model of EXPERIMENTAL_ASR_MODELS) {
      let unregister: (() => void) | undefined;
      refs[model] = (anchor) => {
        unregister?.();
        unregister = anchor ? store.register(model, anchor) : undefined;
      };
    }
    return refs;
  }, [store]);
}

export function useExperimentalDownloadOwners() {
  const store = useOwners();
  useSyncExternalStore(store.subscribe, store.getSnapshot);
  // Read live on click as well as on render; the owner's row may have just unmounted.
  return store.find;
}
