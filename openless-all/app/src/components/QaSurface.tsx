import { useLayoutEffect, useRef, type ReactNode } from 'react';
import { useReducedMotion } from '../lib/motion';

interface Props {
  className: string;
  closing: boolean;
  ready: boolean;
  expanded: boolean;
  children: ReactNode;
}

/** Keep the painted result alive until the native 240ms hide completes. */
export function QaSurface({ closing, ready, ...next }: Props) {
  const frozen = useRef(next);
  if (!closing) frozen.current = next;
  const presentation = closing ? frozen.current : next;
  const ref = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();
  const painted = useRef<{ opacity: string; transform: string } | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element || (!ready && !closing)) return;
    if (reduced) {
      element.style.opacity = closing ? '0' : '1';
      return;
    }
    const hidden = {
      opacity: 0,
      transform: presentation.expanded ? 'translateY(6px) scale(0.975)' : 'translateY(5px)',
    };
    const current = painted.current ?? getComputedStyle(element);
    painted.current = null;
    const animation = element.animate(
      closing
        ? [{ opacity: current.opacity, transform: current.transform }, hidden]
        : [hidden, { opacity: 1, transform: 'none' }],
      {
        duration: closing ? 160 : 180,
        easing: closing ? 'cubic-bezier(0.4,0,1,1)' : 'cubic-bezier(0.16,1,0.3,1)',
        fill: 'both',
      },
    );
    animation.id = closing ? 'qa-result-exit' : 'qa-result-enter';
    return () => {
      const style = getComputedStyle(element);
      painted.current = { opacity: style.opacity, transform: style.transform };
      animation.cancel();
    };
  }, [closing, ready, presentation.expanded, reduced]);
  return (
    <div
      ref={ref}
      className={`${presentation.className.replace(' is-closing', '')}${closing ? ' is-closing' : ''}`}
      style={{
        visibility: ready || closing ? 'visible' : 'hidden',
        transformOrigin: 'center bottom',
      }}
    >
      {presentation.children}
    </div>
  );
}
