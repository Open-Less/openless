import {
  Suspense,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from 'react';
import { useReducedMotion } from '../../lib/motion';
import { useSmoothWheelScroll } from '../../lib/useSmoothWheelScroll';
import { PageSkeleton } from './PageSkeleton';

const easing = 'cubic-bezier(0.22, 1, 0.36, 1)';
const settled = { opacity: 1, transform: 'none', filter: 'none' };
const waiting = { opacity: 0, transform: 'translateY(12px) scale(0.975)', filter: 'blur(8px)' };
const departing = { opacity: 0, transform: 'translateY(-12px) scale(0.985)', filter: 'blur(6px)' };

type Surface = { key: string; children: ReactNode };
interface Props {
  contentKey: string;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  scrollRef?: RefObject<HTMLDivElement>;
  page?: string;
  pace?: 'default' | 'gentle';
  onEntered?: () => void;
}

/** Retain the live outgoing page until the next module and its first useful frame
 * are ready. Both surfaces share one fixed viewport, so navigation never changes
 * document height. New requests supersede the pending destination immediately. */
export function TransitionViewport({
  contentKey,
  children,
  className,
  style,
  scrollRef,
  page,
  pace = 'default',
  onEntered,
}: Props) {
  const reduced = useReducedMotion();
  const [committed, setCommitted] = useState<Surface>({ key: contentKey, children });
  const retained = useRef(committed);
  const latest = useRef({ key: contentKey, children });
  latest.current = { key: contentKey, children };
  const pendingSince = useRef(performance.now());
  const pendingKey = useRef(contentKey);
  if (pendingKey.current !== contentKey) {
    pendingKey.current = contentKey;
    pendingSince.current = performance.now();
  }
  const [slowKey, setSlowKey] = useState<string | null>(null);
  const entered = useRef(onEntered);
  entered.current = onEntered;
  const [readyKey, setReadyKey] = useState<string | null>(null);
  const switching = committed.key !== contentKey;
  const revealing = switching && readyKey === contentKey;
  const restoring = readyKey !== null && readyKey !== contentKey;

  useLayoutEffect(() => {
    if (!switching || reduced) retained.current = { key: contentKey, children };
    if (reduced && switching) setCommitted({ key: contentKey, children });
    if (reduced && readyKey !== null) setReadyKey(null);
  }, [switching, contentKey, children, reduced, readyKey]);
  useEffect(() => {
    if (reduced) entered.current?.();
  }, [contentKey, reduced]);
  const commit = (key: string) => {
    if (latest.current.key !== key) return;
    retained.current = latest.current;
    setCommitted(latest.current);
    setReadyKey(null);
    requestAnimationFrame(() => {
      if (latest.current.key === key) entered.current?.();
    });
  };
  const currentSurface: Surface = { key: contentKey, children };
  const surfaces = switching && !reduced ? [retained.current, currentSurface] : [currentSurface];

  return (
    <div
      className="ol-transition-viewport"
      aria-busy={(!reduced && (switching || restoring)) || undefined}
    >
      {surfaces.map((surface) => {
        const incoming = !reduced && switching && surface.key === contentKey;
        const leaving = !reduced && revealing && !incoming;
        const phase = incoming
          ? revealing
            ? 'enter'
            : 'waiting'
          : leaving
            ? 'exit'
            : !reduced && restoring
              ? 'restore'
              : 'settled';
        return (
          <TransitionSurface
            key={surface.key}
            surfaceKey={surface.key}
            phase={phase}
            reduced={reduced}
            slow={slowKey === contentKey}
            pace={pace}
            className={className}
            style={style}
            scrollRef={surface.key === contentKey ? scrollRef : undefined}
            page={surface.key === contentKey ? page : undefined}
            onReady={() => {
              if (latest.current.key === surface.key) {
                setSlowKey(performance.now() - pendingSince.current > 160 ? surface.key : null);
                setReadyKey(surface.key);
              }
            }}
            onEntered={() => commit(surface.key)}
          >
            {surface.children}
          </TransitionSurface>
        );
      })}
      <div className="ol-transition-mask" aria-hidden="true" />
    </div>
  );
}

function TransitionSurface({
  surfaceKey,
  phase,
  reduced,
  slow,
  pace,
  children,
  className,
  style,
  scrollRef,
  page,
  onReady,
  onEntered,
}: {
  surfaceKey: string;
  phase: 'settled' | 'waiting' | 'enter' | 'exit' | 'restore';
  reduced: boolean;
  slow: boolean;
  pace: 'default' | 'gentle';
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  scrollRef?: RefObject<HTMLDivElement>;
  page?: string;
  onReady: () => void;
  onEntered: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const completed = useRef(onEntered);
  completed.current = onEntered;
  const initialPose = useRef(phase === 'waiting' ? waiting : settled);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    let cancelled = false;
    const target = phase === 'waiting' ? waiting : phase === 'exit' ? departing : settled;
    const apply = () => {
      node.style.opacity = String(target.opacity);
      node.style.transform = target.transform;
      node.style.filter = target.filter;
      node.style.willChange = '';
    };
    if (
      reduced ||
      phase === 'waiting' ||
      phase === 'settled' ||
      typeof node.animate !== 'function'
    ) {
      apply();
      if (phase === 'enter' || phase === 'restore') completed.current();
      return;
    }
    const current = getComputedStyle(node);
    const from = { opacity: current.opacity, transform: current.transform, filter: current.filter };
    node.style.willChange = 'transform, opacity, filter';
    // One native timeline keeps opacity, scale and blur synchronized even when
    // JavaScript frames are delayed. Cleanup freezes the painted pose for reversals.
    const timingScale = pace === 'gentle' ? 1.2 : 1;
    const animation = node.animate([from, target], {
      duration: (phase === 'exit' ? (slow ? 220 : 160) : slow ? 260 : 180) * timingScale,
      delay: (phase === 'enter' ? (slow ? 120 : 70) : 0) * timingScale,
      easing,
      fill: 'both',
    });
    animation.id = `ol-page-${phase}`;
    void animation.finished
      .then(() => {
        if (cancelled) return;
        apply();
        animation.cancel();
        if (phase === 'enter' || phase === 'restore') completed.current();
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      const painted = getComputedStyle(node);
      const pose = {
        opacity: painted.opacity,
        transform: painted.transform,
        filter: painted.filter,
      };
      Object.assign(node.style, pose);
      animation.cancel();
      node.style.willChange = '';
    };
  }, [phase, reduced, slow, pace]);
  useSmoothWheelScroll(ref, surfaceKey);
  useEffect(() => {
    const node = ref.current;
    if (!node || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => {
      const active = document.activeElement;
      if (!(active instanceof HTMLElement) || !node.contains(active)) return;
      const viewport = node.getBoundingClientRect();
      const target = active.getBoundingClientRect();
      if (target.top < viewport.top || target.bottom > viewport.bottom) {
        active.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'auto' });
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [surfaceKey]);
  useLayoutEffect(() => {
    const node = ref.current;
    if (node) node.inert = phase !== 'settled';
    if (scrollRef) (scrollRef as { current: HTMLDivElement | null }).current = node;
  }, [phase, scrollRef]);
  useEffect(() => {
    if (reduced && phase === 'enter') completed.current();
  }, [phase, reduced]);
  return (
    <div
      ref={ref}
      className={className}
      data-ol-page={page}
      data-transition-phase={phase}
      aria-hidden={phase !== 'settled' || undefined}
      style={{
        ...style,
        ...initialPose.current,
        position: 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        boxSizing: 'border-box',
        transformOrigin: '50% 30%',
        pointerEvents: phase !== 'settled' ? 'none' : undefined,
      }}
    >
      <Suspense fallback={phase === 'settled' ? <PageSkeleton /> : null}>
        {children}
        {phase === 'waiting' && <ReadyFrame container={ref} onReady={onReady} />}
      </Suspense>
    </div>
  );
}

function ReadyFrame({
  container,
  onReady,
}: {
  container: RefObject<HTMLDivElement>;
  onReady: () => void;
}) {
  const callback = useRef(onReady);
  callback.current = onReady;
  useEffect(() => {
    const node = container.current;
    if (!node) return;
    let frame = 0;
    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true;
      observer.disconnect();
      window.clearTimeout(deadline);
      callback.current();
    };
    const check = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (
          node.childElementCount > 0 &&
          !node.querySelector('[aria-busy="true"], .ol-page-skeleton, .ol-loading-spinner')
        )
          finish();
      });
    };
    const observer = new MutationObserver(check);
    observer.observe(node, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['aria-busy', 'class'],
    });
    // Long network operations keep their local loading UI instead of freezing navigation.
    const deadline = window.setTimeout(finish, 1200);
    frame = requestAnimationFrame(check);
    return () => {
      finished = true;
      observer.disconnect();
      clearTimeout(deadline);
      cancelAnimationFrame(frame);
    };
  }, [container]);
  return null;
}
