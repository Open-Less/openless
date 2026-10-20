import {
  useEffect,
  useRef,
  type PointerEvent as ReactPointerEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import { flushSync } from 'react-dom';

export function insertionSlot(top: number, slots: number[]) {
  if (top <= slots[0]) return 0;
  for (let i = 0; i < slots.length - 1; i++) {
    if (top < slots[i + 1]) return i + (top - slots[i]) / Math.max(1, slots[i + 1] - slots[i]);
  }
  return slots.length - 1;
}
export function displacedTop(base: number, index: number, slot: number, space: number) {
  return base + space * Math.max(0, Math.min(1, index + 1 - slot));
}

/** A temporary absolute stack: pointer and spring updates never re-render React.
 * Every frame computes a fractional gap, rather than swapping entire rows at a threshold.
 * Pointer-cancel/Escape restores the original order; only a completed drop persists.
 */
export function useSpringReorder<T extends { id: string }>(
  items: T[],
  commit: (next: T[]) => void,
) {
  const rows = useRef(new Map<string, HTMLDivElement>());
  const list = useRef<HTMLDivElement>(null);
  const current = useRef({ items, commit });
  current.current = { items, commit };
  const cleanup = useRef<(() => void) | null>(null);
  useEffect(() => () => cleanup.current?.(), []);
  // A background refresh invalidates this gesture's snapshot; never overwrite newer data.
  useEffect(() => {
    cleanup.current?.();
  }, [items]);

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>, id: string) => {
    if (event.button !== 0 || !event.isPrimary || cleanup.current) return;
    const container = list.current;
    const dragged = rows.current.get(id);
    if (!container || !dragged || items.length < 2) return;
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const pointerId = event.pointerId;
    const original = current.current.items.slice();
    const rect = container.getBoundingClientRect();
    const scale = rect.width / container.offsetWidth || 1;
    const gap = parseFloat(getComputedStyle(container).rowGap) || 0;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    const poses = original.map((item) => {
      const element = rows.current.get(item.id)!;
      const properties = ['position', 'top', 'left', 'width', 'transition', 'transform'] as const;
      const restore = () =>
        properties.forEach((property) => {
          element.style[property] = saved[property];
        });
      const saved = Object.fromEntries(
        properties.map((property) => [property, element.style[property]]),
      );
      return {
        id: item.id,
        element,
        restore,
        height: element.offsetHeight,
        y: (element.getBoundingClientRect().top - rect.top) / scale,
        velocity: 0,
        target: 0,
      };
    });
    const active = poses.find((pose) => pose.id === id)!;
    const others = poses.filter((pose) => pose !== active);
    const originalIndex = poses.indexOf(active);
    const startTop = Math.min(...poses.map((pose) => pose.y));
    const slots = [startTop];
    for (const pose of others) slots.push(slots[slots.length - 1] + pose.height + gap);
    const grab = (event.clientY - rect.top) / scale - active.y;
    const containerCss = container.style.cssText;
    container.style.height = `${container.offsetHeight}px`;
    container.style.position = 'relative';
    let scrollParent: HTMLElement | null = container.parentElement;
    while (scrollParent && !/auto|scroll/.test(getComputedStyle(scrollParent).overflowY))
      scrollParent = scrollParent.parentElement;
    const oldCursor = document.body.style.cursor;
    document.body.style.cursor = 'grabbing';
    for (const pose of poses) {
      Object.assign(pose.element.style, {
        position: 'absolute',
        top: '0',
        left: '0',
        width: '100%',
        transition: 'none',
        transform: `translateY(${pose.y}px)`,
      });
      pose.element.dataset.reordering = pose === active ? 'active' : 'sibling';
    }
    let pointerY = event.clientY,
      slot = originalIndex,
      released = false,
      cancelled = false;
    let frame = 0,
      previous = performance.now(),
      releaseAt = 0;
    const finish = () => {
      const destination = Math.round(slot);
      cleanup.current?.();
      if (!cancelled && destination !== originalIndex) {
        const next = original.filter((item) => item.id !== id);
        next.splice(destination, 0, original[originalIndex]);
        flushSync(() => current.current.commit(next));
      }
    };
    const tick = (now: number) => {
      const dt = Math.min(0.032, Math.max(0.001, (now - previous) / 1000));
      previous = now;
      if (!released) {
        if (scrollParent) {
          const bounds = scrollParent.getBoundingClientRect();
          const edge = 40;
          const speed =
            pointerY < bounds.top + edge
              ? -Math.min(1, (bounds.top + edge - pointerY) / edge)
              : pointerY > bounds.bottom - edge
                ? Math.min(1, (pointerY - bounds.bottom + edge) / edge)
                : 0;
          scrollParent.scrollTop += speed * dt * 480;
        }
        active.y = Math.max(
          startTop - 12,
          Math.min(
            slots[slots.length - 1] + 12,
            (pointerY - container.getBoundingClientRect().top) / scale - grab,
          ),
        );
        slot = insertionSlot(active.y, slots);
      }
      others.forEach((pose, index) => {
        pose.target = displacedTop(slots[index], index, slot, active.height + gap);
      });
      active.target = slots[Math.round(slot)];
      let settled = true;
      for (const pose of poses) {
        if (pose !== active || released) {
          if (reduced) {
            pose.y = pose.target;
            pose.velocity = 0;
          } else {
            const steps = Math.ceil(dt / 0.008),
              step = dt / steps;
            for (let i = 0; i < steps; i++) {
              pose.velocity += ((pose.target - pose.y) * 480 - pose.velocity * 44) * step;
              pose.y += pose.velocity * step;
            }
          }
          if (Math.abs(pose.target - pose.y) > 0.15 || Math.abs(pose.velocity) > 1) settled = false;
        }
        pose.element.style.transform = `translateY(${pose.y}px)${pose === active && !reduced ? ' scale(1.008)' : ''}`;
      }
      if (released && (settled || now - releaseAt > 600)) finish();
      else frame = requestAnimationFrame(tick);
    };
    const move = (next: PointerEvent) => {
      if (next.pointerId === pointerId) pointerY = next.clientY;
    };
    const release = (cancel: boolean) => {
      if (released) return;
      released = true;
      cancelled = cancel;
      releaseAt = performance.now();
      slot = cancel ? originalIndex : Math.round(slot);
    };
    const up = (next: PointerEvent) => {
      if (released || next.pointerId !== pointerId) return;
      slot = insertionSlot(
        (next.clientY - container.getBoundingClientRect().top) / scale - grab,
        slots,
      );
      release(false);
    };
    const cancel = () => release(true);
    const pointerCancel = (next: PointerEvent) => {
      if (next.pointerId === pointerId) cancel();
    };
    const key = (next: KeyboardEvent) => {
      if (next.key === 'Escape') {
        next.preventDefault();
        next.stopPropagation();
        cancel();
      }
    };
    const swallow = (next: MouseEvent) => {
      next.preventDefault();
      next.stopPropagation();
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', pointerCancel);
    window.addEventListener('blur', cancel);
    window.addEventListener('keydown', key, true);
    handle.addEventListener('click', swallow, true);
    cleanup.current = () => {
      cancelAnimationFrame(frame);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', pointerCancel);
      window.removeEventListener('blur', cancel);
      window.removeEventListener('keydown', key, true);
      handle.removeEventListener('click', swallow, true);
      if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId);
      for (const pose of poses) {
        pose.restore();
        delete pose.element.dataset.reordering;
      }
      container.style.cssText = containerCss;
      document.body.style.cursor = oldCursor;
      cleanup.current = null;
    };
    frame = requestAnimationFrame(tick);
  };
  const onKeyDown = (event: ReactKeyboardEvent<HTMLElement>, id: string) => {
    if (!['ArrowUp', 'ArrowDown'].includes(event.key) || cleanup.current) return;
    event.preventDefault();
    event.stopPropagation();
    const next = current.current.items.slice();
    const index = next.findIndex((item) => item.id === id);
    const to = Math.max(0, Math.min(next.length - 1, index + (event.key === 'ArrowUp' ? -1 : 1)));
    if (index === to) return;
    next.splice(to, 0, next.splice(index, 1)[0]);
    current.current.commit(next);
  };
  return { rows, list, onPointerDown, onKeyDown };
}
