import { useLayoutEffect, useRef, type ReactNode } from 'react';
import type { ToastVariant } from '../../api/interaction';
import { useReducedMotion } from '../../lib/motion';
import { useAppViewport } from '../layout/AppViewport';
import './toast.css';

interface StatusNoticeProps {
  children: ReactNode;
  state: string;
  centered?: boolean;
  progress?: boolean;
  notificationKey?: string;
  variant: ToastVariant;
  severity?: 'warning' | 'error';
  present?: boolean;
  onExit?: () => void;
}

type NoticeFrame = { transform: string; opacity: string };
const visible: NoticeFrame = { transform: 'none', opacity: '1' };

function paintedFrame(card: HTMLElement): NoticeFrame {
  const { transform, opacity } = getComputedStyle(card);
  return { transform, opacity };
}

/** A non-modal status surface; the viewport owns clipping and the animation owns removal. */
export function StatusNotice({
  children,
  state,
  centered,
  progress,
  severity,
  variant,
  notificationKey,
  present = true,
  onExit,
}: StatusNoticeProps) {
  const ref = useRef<HTMLDivElement>(null);
  const { boundary } = useAppViewport();
  const reduced = useReducedMotion();
  const exit = useRef(onExit);
  exit.current = onExit;
  const interrupted = useRef<NoticeFrame | null>(null);

  useLayoutEffect(() => {
    if (ref.current) ref.current.inert = !present;
  }, [present]);

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
    let previousSize = '';
    const stop = () => {
      if (!animation) return;
      animation.onfinish = null;
      animation.cancel();
      animation = undefined;
    };
    const run = (initial: boolean) => {
      const from = animation ? paintedFrame(card) : interrupted.current;
      interrupted.current = null;
      stop();
      const box = card.getBoundingClientRect();
      const style = getComputedStyle(card);
      // CSS zoom changes the painted distance, while translateY is in local CSS pixels.
      const scale = box.height / parseFloat(style.height) || 1;
      const top = boundary?.getBoundingClientRect().top ?? 0;
      const hidden: NoticeFrame = {
        transform: `translate3d(0, ${Math.min(-2, (top - box.bottom - 2) / scale)}px, 0)`,
        opacity: present ? '1' : '0',
      };
      previousSize = `${box.width}:${box.height}:${boundary?.clientHeight}`;
      animation = card.animate(
        [from ?? (initial && present ? hidden : visible), present ? visible : hidden],
        {
          duration: present ? 220 : 230,
          easing: present ? 'cubic-bezier(0.16, 1, 0.3, 1)' : 'cubic-bezier(0.28, 0.5, 0.57, 1)',
          fill: 'both',
        },
      );
      animation.id = present ? 'ol-notice-enter' : 'ol-notice-exit';
      const current = animation;
      // A retargeted or unmounted card must not keep a pending finished callback.
      current.onfinish = () => {
        current.onfinish = null;
        if (cancelled || animation !== current || !card.isConnected) return;
        if (present) stop();
        else {
          observer.disconnect();
          exit.current?.();
        }
      };
    };
    const observer = new ResizeObserver(() => {
      if (cancelled) return;
      const box = card.getBoundingClientRect();
      if (`${box.width}:${box.height}:${boundary?.clientHeight}` !== previousSize && animation)
        run(false);
    });
    run(true);
    observer.observe(card);
    if (boundary) observer.observe(boundary);
    return () => {
      cancelled = true;
      interrupted.current = animation && card.isConnected ? paintedFrame(card) : null;
      observer.disconnect();
      stop();
    };
  }, [present, reduced, boundary]);

  return (
    <div
      ref={ref}
      className={`ol-notification-card${progress ? ' ol-notification-card--progress' : ''}`}
      data-state={state}
      data-centered={centered || undefined}
      data-type={variant}
      data-notification-key={notificationKey}
      data-variant={variant === 'error' || variant === 'warning' ? 'alert' : 'default'}
      data-severity={severity ?? (state === 'failed' ? 'error' : undefined)}
      aria-hidden={!present || undefined}
    >
      {children}
    </div>
  );
}
