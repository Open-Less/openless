import { useEffect, type RefObject } from 'react';
import { useReducedMotion } from './motion';

export function discreteWheelDelta(
  deltaY: number,
  deltaMode: number,
  pageHeight: number,
): number | null {
  if (deltaMode === 1) return deltaY * 18;
  if (deltaMode === 2) return deltaY * pageHeight;
  const amount = Math.abs(deltaY);
  return amount >= 40 && (amount % 40 === 0 || amount % 100 === 0) ? deltaY : null;
}

/** Smooth coarse mouse-wheel notches only; trackpad momentum, touch, zoom,
 * horizontal gestures and nested scroll controls remain native. */
export function useSmoothWheelScroll(ref: RefObject<HTMLElement>, key = '') {
  const reduced = useReducedMotion();
  useEffect(() => {
    const node = ref.current;
    if (!node || reduced) return;
    let frame = 0;
    let target = node.scrollTop;
    let previous = 0;
    const cancel = () => {
      cancelAnimationFrame(frame);
      frame = 0;
      target = node.scrollTop;
    };
    const tick = (time: number) => {
      const dt = Math.min(48, time - previous || 16);
      previous = time;
      target = Math.max(0, Math.min(target, node.scrollHeight - node.clientHeight));
      const distance = target - node.scrollTop;
      if (Math.abs(distance) < 1.5) {
        node.scrollTop = target;
        frame = 0;
        return;
      }
      node.scrollTop += distance * (1 - Math.exp(-dt / 45));
      frame = requestAnimationFrame(tick);
    };
    const wheel = (event: WheelEvent) => {
      if (
        event.defaultPrevented ||
        event.ctrlKey ||
        event.metaKey ||
        event.shiftKey ||
        Math.abs(event.deltaX) > Math.abs(event.deltaY)
      )
        return;
      const delta = discreteWheelDelta(event.deltaY, event.deltaMode, node.clientHeight);
      if (delta === null) {
        cancel();
        return;
      }
      for (const targetNode of event.composedPath()) {
        if (targetNode === node) break;
        if (
          targetNode instanceof HTMLElement &&
          targetNode.scrollHeight > targetNode.clientHeight + 1 &&
          /auto|scroll/.test(getComputedStyle(targetNode).overflowY)
        )
          return;
      }
      const maximum = node.scrollHeight - node.clientHeight;
      if (
        maximum <= 0 ||
        (node.scrollTop <= 0 && delta < 0) ||
        (node.scrollTop >= maximum - 1 && delta > 0)
      )
        return;
      event.preventDefault();
      if (!frame) target = node.scrollTop;
      target = Math.max(0, Math.min(maximum, target + delta));
      if (!frame) {
        previous = performance.now();
        frame = requestAnimationFrame(tick);
      }
    };
    node.addEventListener('wheel', wheel, { passive: false });
    node.addEventListener('pointerdown', cancel, { passive: true });
    node.addEventListener('touchstart', cancel, { passive: true });
    node.addEventListener('keydown', cancel);
    return () => {
      cancel();
      node.removeEventListener('wheel', wheel);
      node.removeEventListener('pointerdown', cancel);
      node.removeEventListener('touchstart', cancel);
      node.removeEventListener('keydown', cancel);
    };
  }, [ref, key, reduced]);
}
