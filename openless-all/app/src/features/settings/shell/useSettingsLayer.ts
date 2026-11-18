import { useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { useOverlayMotion } from '../../../lib/motion';
import { useAppViewport } from '../../../ui/layout/AppViewport';
import { DIALOG_LAYER, type ModalLayer } from '../../../ui/overlays/modalStack';

export function useSettingsLayer({
  closing,
  initialFocus,
  mobile,
  onEscape,
  onPaintedChange,
  overlayRef,
  surfaceRef,
}: {
  closing: boolean;
  initialFocus: RefObject<HTMLElement>;
  mobile: boolean;
  onEscape: () => void;
  onPaintedChange?: (painted: boolean) => void;
  overlayRef: RefObject<HTMLDivElement>;
  surfaceRef: RefObject<HTMLDivElement>;
}) {
  const { boundary, modalStack } = useAppViewport();
  const [popupHost, setPopupHost] = useState<HTMLDivElement | null>(null);
  const priority = mobile ? 70 : DIALOG_LAYER;
  const callbacks = useRef({ closing, initialFocus, onEscape, priority });
  callbacks.current = { closing, initialFocus, onEscape, priority };
  const registered = useRef<ModalLayer | null>(null);
  const returnFocus = useRef(modalStack.getReturnFocus());

  useOverlayMotion(overlayRef, closing, 'backdrop', true, 'settings');
  useLayoutEffect(() => {
    if (!overlayRef.current || !surfaceRef.current) return;
    const layer: ModalLayer = {
      card: surfaceRef.current,
      overlay: overlayRef.current,
      close: () => callbacks.current.onEscape(),
      focus: () => callbacks.current.initialFocus.current,
      returnFocus: returnFocus.current,
      closing: callbacks.current.closing,
      priority: callbacks.current.priority,
    };
    registered.current = layer;
    const unregister = modalStack.register(layer);
    return () => {
      registered.current = null;
      unregister();
    };
  }, [modalStack, overlayRef, surfaceRef]);

  useLayoutEffect(() => {
    const layer = registered.current;
    if (!layer) return;
    layer.closing = closing;
    layer.priority = priority;
    modalStack.refresh();
  }, [modalStack, closing, priority]);

  useLayoutEffect(() => {
    onPaintedChange?.(false);
    if (closing) return;
    let active = true;
    const animations = overlayRef.current?.getAnimations?.() ?? [];
    void Promise.allSettled(animations.map((animation) => animation.finished)).then(() => {
      if (active) onPaintedChange?.(true);
    });
    return () => {
      active = false;
      onPaintedChange?.(false);
    };
  }, [closing, onPaintedChange, overlayRef]);

  return { boundary, popupHost, setPopupHost, priority };
}
