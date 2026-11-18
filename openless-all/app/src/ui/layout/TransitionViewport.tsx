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
import { PageSkeleton } from '../../components/ui/PageSkeleton';
import './viewport.css';

const crossfadeEasing = 'cubic-bezier(0.22, 1, 0.36, 1)';
const pageEasing = 'cubic-bezier(0.4, 0, 0.2, 1)';
const settled = { opacity: 1, transform: 'none', filter: 'none' };
const hidden = { ...settled, opacity: 0 };
const blurred = { ...hidden, filter: 'blur(3px)' };

interface Props {
  contentKey: string;
  children: ReactNode;
  header?: ReactNode;
  className?: string;
  style?: CSSProperties;
  scrollRef?: RefObject<HTMLDivElement>;
  page?: string;
  pace?: 'default' | 'gentle';
  mode?: 'crossfade' | 'page';
  onEntered?: () => void;
}
type Surface = Pick<Props, 'children' | 'header' | 'className' | 'style' | 'page'> & {
  key: string;
};
type Phase = 'settled' | 'retained' | 'waiting' | 'enter' | 'exit' | 'restore';
type Handoff = { requestId: number; stage: 'exit' | 'enter' };

/** Retain the live outgoing page until the next module and its first useful frame
 * are ready. Both surfaces share one fixed viewport, so navigation never changes
 * document height. New requests supersede the pending destination immediately. */
export function TransitionViewport({
  contentKey,
  children,
  header,
  className,
  style,
  scrollRef,
  page,
  pace = 'default',
  mode = 'crossfade',
  onEntered,
}: Props) {
  const reduced = useReducedMotion();
  const request: Surface = { key: contentKey, children, header, className, style, page };
  const [committed, setCommitted] = useState<Surface>(request);
  const retained = useRef(committed);
  // A generation distinguishes A → B → A requests from late callbacks for the first A.
  const [target, setTarget] = useState({ key: contentKey, id: 0 });
  if (target.key !== contentKey) setTarget({ key: contentKey, id: target.id + 1 });
  const latest = useRef({ request, id: target.id });
  const entered = useRef(onEntered);
  entered.current = onEntered;
  const lastRequested = useRef(contentKey);
  const pendingEntry = useRef(reduced);
  const [handoff, setHandoff] = useState<Handoff | null>(null);
  const switching = committed.key !== contentKey;
  const revealing = switching && handoff?.requestId === target.id;
  const entering = revealing && (mode === 'crossfade' || handoff?.stage === 'enter');
  const restoring = handoff !== null && handoff.requestId !== target.id;

  useLayoutEffect(() => {
    latest.current = { request, id: target.id };
    if (!switching) retained.current = request;
    if (reduced && switching) setCommitted(request);
    if (reduced && handoff !== null) setHandoff(null);
  });
  useLayoutEffect(() => {
    if (lastRequested.current !== contentKey) {
      lastRequested.current = contentKey;
      pendingEntry.current = true;
    }
    if (!switching && !restoring && pendingEntry.current) {
      // Child layout effects have cleared inert by the time focus can be restored.
      pendingEntry.current = false;
      entered.current?.();
    }
  }, [contentKey, switching, restoring]);
  // An animation can queue a commit just as navigation returns to the retained page.
  // Only reuse the cached children when their key still belongs to the outgoing state.
  const outgoing = retained.current.key === committed.key ? retained.current : committed;
  const surfaces = switching && !reduced ? [outgoing, request] : [request];

  return (
    <div
      className="ol-transition-viewport"
      data-ol-transition={mode}
      aria-busy={(!reduced && (switching || restoring)) || undefined}
    >
      {surfaces.map((surface) => {
        const incoming = !reduced && switching && surface.key === contentKey;
        const leaving = !reduced && revealing && !incoming;
        const phase = incoming
          ? entering
            ? 'enter'
            : 'waiting'
          : leaving
            ? 'exit'
            : !reduced && restoring
              ? 'restore'
              : !reduced && switching && mode === 'page'
                ? 'retained'
                : 'settled';
        return (
          <TransitionSurface
            key={surface.key}
            surfaceKey={surface.key}
            phase={phase}
            reduced={reduced}
            pace={pace}
            mode={mode}
            className={surface.className}
            style={surface.style}
            header={surface.header}
            scrollRef={surface.key === contentKey ? scrollRef : undefined}
            page={surface.page}
            onReady={() => {
              if (latest.current.id === target.id) {
                setHandoff({ requestId: target.id, stage: 'exit' });
              }
            }}
            onEntered={() => {
              if (latest.current.id !== target.id) return;
              if (incoming) {
                setCommitted(latest.current.request);
                setHandoff(null);
              } else {
                setHandoff((current) => (current === handoff ? null : current));
              }
            }}
            onExited={() => {
              if (latest.current.id === target.id && mode === 'page') {
                setHandoff({ requestId: target.id, stage: 'enter' });
              }
            }}
          >
            {surface.children}
          </TransitionSurface>
        );
      })}
    </div>
  );
}

function TransitionSurface({
  surfaceKey,
  phase,
  reduced,
  pace,
  mode,
  children,
  header,
  className,
  style,
  scrollRef,
  page,
  onReady,
  onEntered,
  onExited,
}: {
  surfaceKey: string;
  phase: Phase;
  reduced: boolean;
  pace: 'default' | 'gentle';
  mode: 'crossfade' | 'page';
  children: ReactNode;
  header?: ReactNode;
  className?: string;
  style?: CSSProperties;
  scrollRef?: RefObject<HTMLDivElement>;
  page?: string;
  onReady: () => void;
  onEntered: () => void;
  onExited: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const completed = useRef(onEntered);
  completed.current = onEntered;
  const exited = useRef(onExited);
  exited.current = onExited;
  const initialPose = useRef(phase === 'waiting' ? hidden : settled);
  const interactive = phase === 'settled';
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    let cancelled = false;
    const target = phase === 'exit' || phase === 'waiting' ? hidden : settled;
    const pageMotion = mode === 'page';
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
      phase === 'retained' ||
      typeof node.animate !== 'function'
    ) {
      apply();
      if (phase === 'enter' || phase === 'restore' || phase === 'exit') {
        // The parent records the accepted request in its own layout effect.
        queueMicrotask(() => {
          if (!cancelled) {
            if (phase === 'exit') exited.current();
            else completed.current();
          }
        });
        return () => {
          cancelled = true;
        };
      }
      return;
    }
    const current = getComputedStyle(node);
    const from = {
      opacity: current.opacity,
      transform: current.transform,
      filter: pageMotion && Number(current.opacity) === 0 ? blurred.filter : current.filter,
    };
    // Filter only this bounded page, never the window or its portal layers.
    node.style.willChange = pageMotion ? 'opacity, filter' : 'opacity';
    const animation = node.animate([from, pageMotion && phase === 'exit' ? blurred : target], {
      duration: pace === 'gentle' || (pageMotion && phase === 'enter') ? 160 : 120,
      easing: pageMotion ? pageEasing : crossfadeEasing,
      fill: 'both',
    });
    animation.id = `ol-page-${phase}`;
    void animation.finished
      .then(() => {
        if (cancelled) return;
        apply();
        animation.cancel();
        if (phase === 'enter' || phase === 'restore') completed.current();
        if (phase === 'exit') exited.current();
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
  }, [phase, reduced, pace, mode]);
  const contentRef = header ? scrollerRef : ref;
  useSmoothWheelScroll(contentRef, surfaceKey);
  useEffect(() => {
    const node = contentRef.current;
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
  }, [surfaceKey, contentRef]);
  useLayoutEffect(() => {
    const node = ref.current;
    if (node) node.inert = !interactive;
    const scroller = contentRef.current;
    if (scrollRef) (scrollRef as { current: HTMLDivElement | null }).current = scroller;
    return () => {
      if (scrollRef?.current === scroller) {
        (scrollRef as { current: HTMLDivElement | null }).current = null;
      }
    };
  }, [interactive, scrollRef, contentRef]);
  return (
    <div
      ref={ref}
      className={header ? 'ol-transition-page-with-header' : className}
      data-ol-page={page}
      data-transition-phase={phase}
      aria-hidden={!interactive || undefined}
      style={{
        ...(header ? undefined : style),
        ...initialPose.current,
        position: 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        boxSizing: 'border-box',
        pointerEvents: !interactive ? 'none' : undefined,
      }}
    >
      {header}
      {header ? (
        <div
          ref={scrollerRef}
          className={className}
          data-transition-phase={phase}
          style={{ ...style, flex: 1, minHeight: 0, boxSizing: 'border-box' }}
        >
          <Suspense fallback={interactive ? <PageSkeleton /> : null}>
            {children}
            {phase === 'waiting' && <ReadyFrame container={contentRef} onReady={onReady} />}
          </Suspense>
        </div>
      ) : (
        <Suspense fallback={interactive ? <PageSkeleton /> : null}>
          {children}
          {phase === 'waiting' && <ReadyFrame container={contentRef} onReady={onReady} />}
        </Suspense>
      )}
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
