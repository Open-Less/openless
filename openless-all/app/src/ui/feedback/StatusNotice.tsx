import { useLayoutEffect, useRef, type ReactNode } from 'react';
import { useReducedMotion } from '../../lib/motion';
import { useAppViewport } from '../layout/AppViewport';
import './status-notice.css';

interface StatusNoticeProps {
  children: ReactNode;
  state: string;
  progress?: boolean;
  severity?: 'warning' | 'error';
  present?: boolean;
  onExit?: () => void;
}

/** A non-modal status surface; the viewport owns clipping and the animation owns removal. */
export function StatusNotice({
  children,
  state,
  progress,
  severity,
  present = true,
  onExit,
}: StatusNoticeProps) {
  const ref = useRef<HTMLDivElement>(null);
  const { boundary } = useAppViewport();
  const reduced = useReducedMotion();
  const exit = useRef(onExit);
  exit.current = onExit;
  const interrupted = useRef<string | null>(null);

  useLayoutEffect(() => {
    const card = ref.current;
    if (!card) return;
    if (reduced || typeof card.animate !== 'function') {
      interrupted.current = null;
      if (present) return;
      let cancelled = false;
      // Let the presence owner register the exit before completing without animation.
      queueMicrotask(() => {
        if (!cancelled) exit.current?.();
      });
      return () => {
        cancelled = true;
      };
    }
    let animation: Animation | undefined;
    let cancelled = false;
    let frame = 0;
    let previousSize = '';
    const run = (initial: boolean) => {
      const painted = getComputedStyle(card).transform;
      const from = animation ? painted : interrupted.current;
      animation?.cancel();
      const box = card.getBoundingClientRect();
      const style = getComputedStyle(card);
      // CSS zoom changes the painted distance, while translateY is in local CSS pixels.
      const scale = box.height / parseFloat(style.height) || 1;
      const top = boundary?.getBoundingClientRect().top ?? 0;
      const hidden = `translate3d(0, ${Math.min(-2, (top - box.bottom - 2) / scale)}px, 0)`;
      previousSize = `${box.width}:${box.height}:${boundary?.clientHeight}`;
      animation = card.animate(
        [
          { transform: from ?? (initial && present ? hidden : 'none') },
          { transform: present ? 'none' : hidden },
        ],
        {
          duration: present ? 220 : 180,
          easing: present ? 'cubic-bezier(0.16, 1, 0.3, 1)' : 'cubic-bezier(0.55, 0, 1, 0.45)',
          fill: 'both',
        },
      );
      animation.id = present ? 'ol-notice-enter' : 'ol-notice-exit';
      const current = animation;
      if (initial && present && !from) {
        current.pause();
        current.currentTime = 0;
        frame = requestAnimationFrame(() => {
          frame = requestAnimationFrame(() => {
            if (!cancelled && animation === current) current.play();
          });
        });
      }
      void current.finished
        .then(() => {
          if (cancelled || animation !== current) return;
          if (present) {
            interrupted.current = null;
            current.cancel();
            animation = undefined;
          } else exit.current?.();
        })
        .catch(() => {});
    };
    run(true);
    const observer = new ResizeObserver(() => {
      if (cancelled) return;
      const box = card.getBoundingClientRect();
      if (`${box.width}:${box.height}:${boundary?.clientHeight}` !== previousSize && animation)
        run(false);
    });
    observer.observe(card);
    if (boundary) observer.observe(boundary);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      interrupted.current = animation ? getComputedStyle(card).transform : null;
      observer.disconnect();
      animation?.cancel();
    };
  }, [present, reduced, boundary]);

  return (
    <div
      ref={ref}
      className={`ol-notification-card${progress ? ' ol-notification-card--progress' : ''}`}
      data-state={state}
      data-variant={severity || state === 'failed' ? 'alert' : 'default'}
      data-severity={severity ?? (state === 'failed' ? 'error' : undefined)}
      aria-hidden={!present || undefined}
    >
      {children}
    </div>
  );
}
