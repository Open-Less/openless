import { useLayoutEffect, useRef, useSyncExternalStore, type RefObject } from 'react';

import { keyboardInteraction } from './interactionMode';

// Presence leaves room for the browser to paint the last frame before removal.
export const OVERLAY_EXIT_MS = 156;
const SURFACE_EXIT_MS = 120;
const SURFACE_ENTER_MS = 168;
const BACKDROP_ENTER_MS = 136;
const CONTENT_ENTER_MS = 148;
const SELECTION_MS = 240;
const SURFACE_EXIT_EASING = 'cubic-bezier(0.4, 0, 0.73, 0.55)';
const SURFACE_ENTER_EASING = 'cubic-bezier(0.22, 1, 0.36, 1)';
const REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)';
let media: MediaQueryList | undefined;

function motionPreference() {
  if (typeof window === 'undefined' || !window.matchMedia) return undefined;
  return (media ??= window.matchMedia(REDUCED_MOTION_QUERY));
}

function subscribeMotionPreference(listener: () => void) {
  const query = motionPreference();
  query?.addEventListener('change', listener);
  return () => query?.removeEventListener('change', listener);
}

export function useReducedMotion() {
  return useSyncExternalStore(
    subscribeMotionPreference,
    () => motionPreference()?.matches ?? false,
    () => false,
  );
}

type Frame = { opacity: string | number; transform: string; width?: string };
const VISIBLE: Frame = { opacity: 1, transform: 'none' };
const FADED: Frame = { opacity: 0, transform: 'none' };
const POP_HIDDEN: Frame = { opacity: 0, transform: 'translate3d(0, 6px, 0) scale(0.94)' };

function currentFrame(element: HTMLElement): Frame {
  const style = getComputedStyle(element);
  return { opacity: style.opacity, transform: style.transform };
}

function animate(
  element: HTMLElement,
  from: Frame,
  to: Frame,
  duration: number,
  exiting: boolean,
  easing = exiting ? SURFACE_EXIT_EASING : SURFACE_ENTER_EASING,
) {
  const previousWillChange = element.style.willChange;
  // A flat page/backdrop fade must not create a transform containing block or
  // promote its text as if it were scaling. Cards/indicators still need both.
  element.style.willChange =
    from.transform === 'none' && to.transform === 'none' ? 'opacity' : 'opacity, transform';
  const animation = element.animate([from, to], { duration, easing, fill: 'both' });
  animation.id = exiting ? 'ol-surface-exit' : 'ol-surface-enter';
  let released = false;
  const releaseLayer = () => {
    if (released) return;
    released = true;
    element.style.willChange = previousWillChange;
  };
  return {
    animation,
    releaseLayer,
    cancel: () => {
      animation.cancel();
      releaseLayer();
    },
  };
}

/** Interrupted overlays resume from their painted pose; no reversed-keyframe restart. */
export function useOverlayMotion(
  ref: RefObject<HTMLElement>,
  closing: boolean,
  variant: 'card' | 'backdrop' | 'sheet' | 'drawer' | 'pop' = 'card',
  enabled = true,
  entrance: 'default' | 'settings' = 'default',
) {
  const reduced = useReducedMotion();
  const interrupted = useRef<{ element: HTMLElement; frame: Frame } | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element || !enabled || reduced || typeof element.animate !== 'function') {
      interrupted.current = null;
      return;
    }
    const settingsEntrance = entrance === 'settings';
    const hidden =
      variant === 'pop'
        ? POP_HIDDEN
        : variant === 'backdrop'
          ? settingsEntrance
            ? { opacity: 0, transform: 'translate3d(0, 10px, 0) scale(0.965)' }
            : FADED
          : {
              opacity: 0,
              transform:
                variant === 'drawer'
                  ? 'translate3d(12px, 0, 0)'
                  : variant === 'sheet'
                    ? 'translate3d(0, 16px, 0)'
                    : settingsEntrance
                      ? 'translate3d(0, 20px, 0) scale(0.96)'
                      : 'translate3d(0, 10px, 0) scale(0.95)',
            };
    const from =
      interrupted.current?.element === element
        ? interrupted.current.frame
        : closing
          ? currentFrame(element)
          : hidden;
    interrupted.current = null;
    const motion = animate(
      element,
      from,
      closing ? hidden : VISIBLE,
      closing
        ? SURFACE_EXIT_MS
        : variant === 'backdrop' && !settingsEntrance
          ? BACKDROP_ENTER_MS
          : SURFACE_ENTER_MS,
      closing,
    );
    let cancelled = false;
    void motion.animation.finished
      .then(() => {
        if (cancelled) return;
        motion.releaseLayer();
        if (!closing) motion.animation.cancel();
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      interrupted.current =
        motion.animation.playState === 'running' || motion.animation.playState === 'paused'
          ? { element, frame: currentFrame(element) }
          : null;
      motion.cancel();
    };
  }, [ref, closing, variant, enabled, entrance, reduced]);
}

/** Reveal replaced content once; a dialog's first content shares its parent's entrance. */
export function useContentMotion(ref: RefObject<HTMLElement>, key: string) {
  const reduced = useReducedMotion();
  const previous = useRef(key);
  const interrupted = useRef<{ element: HTMLElement; frame: Frame } | null>(null);
  useLayoutEffect(() => {
    const changed = previous.current !== key;
    previous.current = key;
    const element = ref.current;
    if (!element || reduced || keyboardInteraction() || typeof element.animate !== 'function') {
      interrupted.current = null;
      return;
    }
    if (!changed) return;
    const from =
      interrupted.current?.element === element
        ? interrupted.current.frame
        : { opacity: 0, transform: 'translate3d(0, 6px, 0)' };
    interrupted.current = null;
    const motion = animate(element, from, VISIBLE, CONTENT_ENTER_MS, false);
    void motion.animation.finished.then(motion.cancel).catch(() => {});
    return () => {
      interrupted.current =
        motion.animation.playState === 'running' || motion.animation.playState === 'paused'
          ? { element, frame: currentFrame(element) }
          : null;
      motion.cancel();
    };
  }, [ref, key, reduced]);
}

/** A shared highlight moves between selected controls without animating their layout. */
export function useSelectionMotion(
  groupRef: RefObject<HTMLElement>,
  indicatorRef: RefObject<HTMLElement>,
  selection: string,
  underline = false,
) {
  const reduced = useReducedMotion();
  const previous = useRef<string | null>(null);
  const interrupted = useRef<Frame | null>(null);
  useLayoutEffect(() => {
    const group = groupRef.current;
    const indicator = indicatorRef.current;
    if (!group || !indicator) return;
    let motion: ReturnType<typeof animate> | undefined;
    let lastTarget = '';
    const position = (moving: boolean) => {
      const button = group.querySelector<HTMLElement>(':scope > button[aria-pressed="true"]');
      if (!button) {
        indicator.style.visibility = 'hidden';
        return;
      }
      // Preserve fractional layout coordinates under CSS zoom and dialog transforms.
      // offsetTop/offsetWidth round half-pixel padding and visibly uncenter the chip.
      const outer = group.getBoundingClientRect();
      const inner = button.getBoundingClientRect();
      const style = getComputedStyle(group);
      const borderX = parseFloat(style.borderLeftWidth) || 0;
      const borderY = parseFloat(style.borderTopWidth) || 0;
      const boxWidth =
        parseFloat(style.width) +
        (style.boxSizing === 'border-box'
          ? 0
          : (parseFloat(style.paddingLeft) || 0) +
            (parseFloat(style.paddingRight) || 0) +
            borderX +
            (parseFloat(style.borderRightWidth) || 0));
      const boxHeight =
        parseFloat(style.height) +
        (style.boxSizing === 'border-box'
          ? 0
          : (parseFloat(style.paddingTop) || 0) +
            (parseFloat(style.paddingBottom) || 0) +
            borderY +
            (parseFloat(style.borderBottomWidth) || 0));
      const scaleX = outer.width / boxWidth || 1;
      const scaleY = outer.height / boxHeight || 1;
      const x = (inner.left - outer.left) / scaleX - borderX;
      const y = underline
        ? boxHeight - borderY - (parseFloat(style.borderBottomWidth) || 0) - 2
        : (inner.top - outer.top) / scaleY - borderY;
      const height = underline ? 2 : inner.height / scaleY;
      const target = `translate3d(${x}px, ${y}px, 0)`;
      const width = `${inner.width / scaleX}px`;
      const targetKey = `${target}:${width}:${height}`;
      if (targetKey === lastTarget) return;
      const from = interrupted.current ?? {
        ...currentFrame(indicator),
        width: getComputedStyle(indicator).width,
      };
      interrupted.current = null;
      motion?.cancel();
      indicator.style.transform = target;
      indicator.style.width = width;
      indicator.style.height = `${height}px`;
      indicator.style.visibility = 'visible';
      lastTarget = targetKey;
      if (moving && !reduced && !keyboardInteraction() && typeof indicator.animate === 'function') {
        motion = animate(
          indicator,
          from,
          { opacity: 1, transform: target, width },
          SELECTION_MS,
          false,
        );
        motion.animation.id = 'ol-selection-move';
        void motion.animation.finished.then(motion.cancel).catch(() => {});
      }
    };
    position(previous.current !== null && previous.current !== selection);
    previous.current = selection;
    let resizeFrame = 0;
    const observer = new ResizeObserver(() => {
      if (resizeFrame) return;
      resizeFrame = requestAnimationFrame(() => {
        resizeFrame = 0;
        position(false);
      });
    });
    observer.observe(group);
    return () => {
      interrupted.current = {
        ...currentFrame(indicator),
        width: getComputedStyle(indicator).width,
      };
      observer.disconnect();
      cancelAnimationFrame(resizeFrame);
      motion?.cancel();
    };
  }, [groupRef, indicatorRef, selection, underline, reduced]);
}
