const FOCUSABLE =
  'a[href], button, textarea, select, input:not([type="hidden"]), [contenteditable="true"], [tabindex]';

export const OPEN_POPUP = ':is(.ol-select-positioner, .ol-tooltip-positioner)[data-open]';
export const DIALOG_LAYER = 50;
export const REQUEST_LAYER = 1200;

export interface ModalLayer {
  overlay: HTMLElement;
  card: HTMLElement;
  close: () => void;
  focus: () => HTMLElement | null;
  returnFocus: HTMLElement | null;
  priority: number;
  closing: boolean;
}

function canFocus(element: HTMLElement | null): element is HTMLElement {
  return (
    !!element?.isConnected &&
    !element.matches(':disabled') &&
    !element.closest('[inert], [hidden], [aria-hidden="true"]') &&
    element.getClientRects().length > 0
  );
}

export function createModalStack() {
  const layers: ModalLayer[] = [];
  const listeners = new Set<() => void>();
  let content: HTMLElement | null = null;
  let top: ModalLayer | undefined;
  let active = false;
  let restoreToPage: HTMLElement | null = null;
  let clickTarget: HTMLElement | null = null;

  function focusLayer(layer: ModalLayer) {
    const preferred = layer.focus();
    (canFocus(preferred) ? preferred : layer.card).focus({ preventScroll: true });
  }

  function refresh() {
    const before = top;
    top = undefined;
    for (const layer of layers) {
      if (!layer.closing && (!top || layer.priority >= top.priority)) top = layer;
    }

    for (const layer of layers) {
      layer.overlay.inert = layer !== top;
      layer.card.setAttribute('aria-modal', String(layer === top));
      if (layer === top) layer.overlay.removeAttribute('aria-hidden');
      else layer.overlay.setAttribute('aria-hidden', 'true');
    }
    if (content) content.inert = !!top;

    if (before !== top) {
      if (top && (!before || (layers.includes(before) && !before.closing))) {
        if (!before) restoreToPage = top.returnFocus;
        if (!top.overlay.contains(document.activeElement)) focusLayer(top);
      } else if (top) {
        const previous = before?.returnFocus ?? null;
        if (top.overlay.contains(previous) && canFocus(previous)) {
          previous.focus({ preventScroll: true });
        } else {
          focusLayer(top);
        }
      } else {
        const previous = canFocus(restoreToPage) ? restoreToPage : (before?.returnFocus ?? null);
        if (canFocus(previous)) previous.focus({ preventScroll: true });
        else if (content && canFocus(content)) content.focus({ preventScroll: true });
        restoreToPage = null;
      }
    }
    if (active !== !!top) {
      active = !!top;
      if (active) {
        document.addEventListener('keydown', onKeydown);
        document.addEventListener('focusin', onFocusIn);
      } else {
        document.removeEventListener('keydown', onKeydown);
        document.removeEventListener('focusin', onFocusIn);
      }
      listeners.forEach((listener) => listener());
    }
  }

  function onFocusIn(event: FocusEvent) {
    if (top && event.target instanceof Node && !top.overlay.contains(event.target)) focusLayer(top);
  }

  function onKeydown(event: KeyboardEvent) {
    if (!top || event.defaultPrevented || event.isComposing) return;
    if (event.key === 'Escape') {
      if (top.overlay.querySelector(OPEN_POPUP)) return;
      event.preventDefault();
      event.stopPropagation();
      top.close();
      return;
    }
    if (event.key !== 'Tab') return;
    // Portaled Base UI controls handle their own focus guards and Escape before
    // the event reaches this listener.
    const { card, overlay } = top;
    const focused = document.activeElement;
    if (overlay.contains(focused) && !card.contains(focused)) return;
    const items = [...card.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
      (element) => element.tabIndex >= 0 && canFocus(element),
    );
    const first = items[0];
    const last = items[items.length - 1];
    if (!first) {
      event.preventDefault();
      card.focus({ preventScroll: true });
    } else if (event.shiftKey && (focused === first || !items.includes(focused as HTMLElement))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (focused === last || !items.includes(focused as HTMLElement))) {
      event.preventDefault();
      first.focus();
    }
  }

  return {
    captureClick(target: EventTarget | null) {
      const control = target instanceof Element ? target.closest<HTMLElement>(FOCUSABLE) : null;
      clickTarget = canFocus(control) ? control : null;
    },
    captureKey() {
      clickTarget = null;
    },
    getReturnFocus() {
      // Safari mouse clicks do not focus buttons. Keep the initiating control
      // for the current surface so dismissal can return to it in that engine.
      const surface = top?.overlay ?? content;
      if (canFocus(clickTarget) && surface?.contains(clickTarget)) return clickTarget;
      return document.activeElement instanceof HTMLElement ? document.activeElement : null;
    },
    focus(layer: ModalLayer) {
      if (top === layer) focusLayer(layer);
    },
    setContent(node: HTMLElement | null) {
      content = node;
      if (content) content.inert = active;
    },
    register(layer: ModalLayer) {
      layers.push(layer);
      refresh();
      return () => {
        const index = layers.indexOf(layer);
        if (index === -1) return;
        layers.splice(index, 1);
        refresh();
      };
    },
    refresh,
    isTop: (layer: ModalLayer) => top === layer,
    getActive: () => active,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

export type ModalStack = ReturnType<typeof createModalStack>;
