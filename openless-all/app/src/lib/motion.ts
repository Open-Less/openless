import { useLayoutEffect, useRef, useState, useSyncExternalStore, type RefObject } from 'react';

import { keyboardInteraction } from './interactionMode';

export const OVERLAY_EXIT_MS = 180;
const ENTER_MS = 240;
const SETTINGS_ENTER_MS = 240;
const SETTINGS_BACKDROP_MS = 220;
const CONTENT_MS = 160;
const CONTENT_EXIT_MS = 120;
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

type Frame = { opacity: string | number; transform: string };
const VISIBLE: Frame = { opacity: 1, transform: 'none' };
const FADED: Frame = { opacity: 0, transform: 'none' };

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
  easingToken = exiting ? '--ol-motion-exit' : '--ol-motion-spring',
) {
  const style = getComputedStyle(element);
  const easing = style.getPropertyValue(easingToken).trim();
  const previousWillChange = element.style.willChange;
  // A flat page/backdrop fade must not create a transform containing block or
  // promote its text as if it were scaling. Cards/indicators still need both.
  element.style.willChange =
    from.transform === 'none' && to.transform === 'none' ? 'opacity' : 'opacity, transform';
  const animation = element.animate([from, to], {
    duration,
    easing: easing || 'cubic-bezier(0.23, 1, 0.32, 1)',
    fill: 'both',
  });
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
  variant: 'card' | 'backdrop' | 'sheet' | 'drawer' = 'card',
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
      variant === 'backdrop'
        ? FADED
        : {
            opacity: 0,
            transform:
              variant === 'drawer'
                ? 'translate3d(12px, 0, 0)'
                : variant === 'sheet'
                  ? 'translate3d(0, 16px, 0)'
                  : settingsEntrance
                    ? 'translate3d(0, 20px, 0) scale(0.96)'
                    : 'translate3d(0, 8px, 0) scale(0.985)',
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
        ? OVERLAY_EXIT_MS
        : settingsEntrance
          ? variant === 'backdrop'
            ? SETTINGS_BACKDROP_MS
            : SETTINGS_ENTER_MS
          : variant === 'backdrop'
            ? CONTENT_MS
            : ENTER_MS,
      closing,
      closing ? '--ol-motion-exit' : settingsEntrance ? '--ol-motion-soft' : '--ol-motion-spring',
    );
    let cancelled = false;
    let entryFrame: number | undefined;
    if (settingsEntrance && !closing) {
      // Paint the mounted dialog at its starting pose before advancing the entrance clock.
      motion.animation.pause();
      motion.animation.currentTime = 0;
      entryFrame = window.requestAnimationFrame(() => {
        if (cancelled) return;
        entryFrame = window.requestAnimationFrame(() => {
          if (!cancelled) motion.animation.play();
        });
      });
    }
    void motion.animation.finished
      .then(() => {
        if (cancelled) return;
        motion.releaseLayer();
        if (!closing) motion.animation.cancel();
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      if (entryFrame !== undefined) window.cancelAnimationFrame(entryFrame);
      interrupted.current = { element, frame: currentFrame(element) };
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
    const motion = animate(element, from, VISIBLE, ENTER_MS, false);
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
      // Layout offsets stay correct while the parent dialog is scaled on entry.
      const y = underline ? group.clientHeight - 2 : button.offsetTop;
      const target = `translate3d(${button.offsetLeft}px, ${y}px, 0) scaleX(${button.offsetWidth / 100})`;
      if (target === lastTarget) return;
      const from = interrupted.current ?? currentFrame(indicator);
      interrupted.current = null;
      motion?.cancel();
      indicator.style.transform = target;
      indicator.style.height = `${underline ? 2 : button.offsetHeight}px`;
      indicator.style.visibility = 'visible';
      lastTarget = target;
      if (moving && !reduced && !keyboardInteraction() && typeof indicator.animate === 'function') {
        motion = animate(indicator, from, { opacity: 1, transform: target }, ENTER_MS, false);
        motion.animation.id = 'ol-selection-move';
        void motion.animation.finished.then(motion.cancel).catch(() => {});
      }
    };
    position(previous.current !== null && previous.current !== selection);
    previous.current = selection;
    const observer = new ResizeObserver(() => position(false));
    observer.observe(group);
    return () => {
      interrupted.current = currentFrame(indicator);
      observer.disconnect();
      motion?.cancel();
    };
  }, [groupRef, indicatorRef, selection, underline, reduced]);
}

/** Keep the outgoing page alive for its fade. Rapid navigation always settles on
 * the latest destination; text is never scaled or translated during a page change. */
export function usePageTransition<T extends string>(
  requested: T,
  ref: RefObject<HTMLElement>,
  _mobile: boolean,
) {
  const [displayed, setDisplayed] = useState(requested);
  const painted = useRef(displayed);
  const interrupted = useRef<{ element: HTMLElement; frame: Frame } | null>(null);
  const reduced = useReducedMotion();
  useLayoutEffect(() => {
    const element = ref.current;
    const entering = displayed === requested;
    const changed = painted.current !== displayed;
    painted.current = displayed;
    if (!element || reduced || typeof element.animate !== 'function') {
      interrupted.current = null;
      if (!entering) setDisplayed(requested);
      return;
    }
    const resume = interrupted.current?.element === element ? interrupted.current.frame : null;
    interrupted.current = null;
    if (entering && !changed && !resume) return;
    const motion = animate(
      element,
      resume ?? (entering ? FADED : currentFrame(element)),
      entering ? VISIBLE : FADED,
      entering ? CONTENT_MS : CONTENT_EXIT_MS,
      !entering,
    );
    element.inert = !entering;
    let cancelled = false;
    void motion.animation.finished
      .then(() => {
        if (cancelled) return;
        motion.releaseLayer();
        if (entering) motion.cancel();
        else setDisplayed(requested);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      interrupted.current =
        motion.animation.playState === 'running' || motion.animation.playState === 'paused'
          ? { element, frame: currentFrame(element) }
          : null;
      element.inert = false;
      motion.cancel();
    };
  }, [requested, displayed, ref, reduced]);
  return displayed;
}
