/** A critically damped spring, expressed in CSS pixels and seconds. */
export function springStep(
  position: number,
  velocity: number,
  target: number,
  dt: number,
  frequency = 15,
) {
  const distance = position - target;
  const impulse = velocity + frequency * distance;
  const decay = Math.exp(-frequency * dt);
  return {
    position: target + (distance + impulse * dt) * decay,
    velocity: (velocity - frequency * impulse * dt) * decay,
  };
}

type Axis = 'x' | 'y';
type Motion = {
  node: HTMLElement;
  axis: Axis;
  position: number;
  velocity: number;
  target: number;
  last: number;
  frame: number;
  originalTranslate: string;
};

function maxScroll(node: HTMLElement, axis: Axis) {
  return Math.max(
    0,
    axis === 'y' ? node.scrollHeight - node.clientHeight : node.scrollWidth - node.clientWidth,
  );
}
function offset(node: HTMLElement, axis: Axis) {
  return axis === 'y' ? node.scrollTop : node.scrollLeft;
}
function setOffset(node: HTMLElement, axis: Axis, value: number) {
  if (axis === 'y') node.scrollTop = value;
  else node.scrollLeft = value;
}
function findScroller(path: EventTarget[], axis: Axis): HTMLElement | null {
  for (const entry of path) {
    if (!(entry instanceof HTMLElement)) continue;
    const style = getComputedStyle(entry);
    if (
      (entry === entry.ownerDocument.scrollingElement ||
        /^(auto|scroll)$/.test(axis === 'y' ? style.overflowY : style.overflowX)) &&
      maxScroll(entry, axis) > 1
    )
      return entry;
  }
  return null;
}

/** Installs one owner for wheel and touch gestures, including nested scroll views. */
export function installSpringScroll(doc: Document = document) {
  const view = doc.defaultView;
  if (!view) return () => {};
  const reduced = view.matchMedia('(prefers-reduced-motion: reduce)');
  let motion: Motion | null = null;
  let touch: {
    node: HTMLElement;
    axis: Axis | null;
    x: number;
    y: number;
    last: number;
    velocity: number;
    moved: boolean;
    path: EventTarget[];
  } | null = null;
  const stop = () => {
    if (!motion) return;
    cancelAnimationFrame(motion.frame);
    motion.node.style.translate = motion.originalTranslate;
    motion = null;
  };
  const paint = (m: Motion) => {
    const maximum = maxScroll(m.node, m.axis);
    const bounded = Math.max(0, Math.min(maximum, m.position));
    setOffset(m.node, m.axis, bounded);
    const overshoot = m.position - bounded;
    const rubber = -Math.sign(overshoot) * Math.min(24, Math.abs(overshoot) * 0.3);
    m.node.style.translate = rubber
      ? m.axis === 'y'
        ? `0 ${rubber}px`
        : `${rubber}px 0`
      : m.originalTranslate;
  };
  const tick = (time: number) => {
    const m = motion;
    if (!m) return;
    if (!m.node.isConnected || reduced.matches) {
      stop();
      return;
    }
    m.target = Math.max(0, Math.min(maxScroll(m.node, m.axis), m.target));
    const next = springStep(
      m.position,
      m.velocity,
      m.target,
      Math.min((time - m.last) / 1000, 0.05),
    );
    m.position = next.position;
    m.velocity = next.velocity;
    m.last = time;
    paint(m);
    if (Math.abs(m.position - m.target) < 0.1 && Math.abs(m.velocity) < 1) {
      setOffset(m.node, m.axis, m.target);
      stop();
      return;
    }
    m.frame = requestAnimationFrame(tick);
  };
  const start = (
    node: HTMLElement,
    axis: Axis,
    position: number,
    target: number,
    velocity: number,
  ) => {
    const originalTranslate =
      motion?.node === node ? motion.originalTranslate : node.style.translate;
    stop();
    motion = {
      node,
      axis,
      position,
      target,
      velocity,
      last: performance.now(),
      frame: 0,
      originalTranslate,
    };
    motion.frame = requestAnimationFrame(tick);
  };
  const wheel = (event: WheelEvent) => {
    if (
      reduced.matches ||
      event.defaultPrevented ||
      event.ctrlKey ||
      event.metaKey ||
      event.shiftKey
    )
      return;
    const axis: Axis = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? 'x' : 'y';
    const node = findScroller(event.composedPath(), axis);
    if (!node) return;
    const delta =
      (axis === 'y' ? event.deltaY : event.deltaX) *
      (event.deltaMode === 1
        ? 18
        : event.deltaMode === 2
          ? axis === 'y'
            ? node.clientHeight
            : node.clientWidth
          : 1);
    if (!delta) return;
    event.preventDefault();
    const current = motion?.node === node && motion.axis === axis ? motion : null;
    const position = current?.position ?? offset(node, axis);
    const target = Math.max(
      0,
      Math.min(maxScroll(node, axis), (current?.target ?? position) + delta),
    );
    const velocity = current?.velocity ?? 0;
    // At an edge, a bounded impulse makes the content yield and return.
    const kick = target === position && !current ? delta * 5 : 0;
    start(node, axis, position, target, velocity + kick);
  };
  const touchStart = (event: TouchEvent) => {
    if (reduced.matches || event.touches.length !== 1 || event.defaultPrevented) return;
    const target = event.target;
    if (
      target instanceof Element &&
      target.closest(
        'input, textarea, select, [contenteditable="true"], [data-spring-scroll-ignore]',
      )
    )
      return;
    stop();
    const point = event.touches[0];
    touch = {
      node: doc.documentElement,
      axis: null,
      x: point.clientX,
      y: point.clientY,
      last: performance.now(),
      velocity: 0,
      moved: false,
      path: event.composedPath(),
    };
  };
  const touchMove = (event: TouchEvent) => {
    if (!touch || event.touches.length !== 1 || event.defaultPrevented) return;
    const point = event.touches[0],
      dx = touch.x - point.clientX,
      dy = touch.y - point.clientY;
    if (!touch.axis && Math.max(Math.abs(dx), Math.abs(dy)) < 4) return;
    if (!touch.axis) {
      touch.axis = Math.abs(dx) > Math.abs(dy) ? 'x' : 'y';
      const node = findScroller(touch.path, touch.axis);
      if (!node) {
        touch = null;
        return;
      }
      touch.node = node;
    }
    if (!event.cancelable) {
      touch = null;
      return;
    }
    event.preventDefault();
    const now = performance.now(),
      delta = touch.axis === 'y' ? dy : dx;
    const dt = Math.max(8, now - touch.last);
    touch.velocity = Math.max(
      -4000,
      Math.min(4000, touch.velocity * 0.35 + (delta / dt) * 1000 * 0.65),
    );
    const previous = motion?.node === touch.node ? motion.position : offset(touch.node, touch.axis);
    const position = Math.max(
      -80,
      Math.min(maxScroll(touch.node, touch.axis) + 80, previous + delta),
    );
    if (!motion)
      motion = {
        node: touch.node,
        axis: touch.axis,
        position,
        velocity: 0,
        target: position,
        last: now,
        frame: 0,
        originalTranslate: touch.node.style.translate,
      };
    motion.position = position;
    paint(motion);
    touch.x = point.clientX;
    touch.y = point.clientY;
    touch.last = now;
    touch.moved = true;
  };
  const touchEnd = () => {
    if (!touch) return;
    if (touch.moved && touch.axis && motion) {
      const elapsed = performance.now() - touch.last;
      const velocity = elapsed > 100 ? 0 : touch.velocity;
      const position = motion.position;
      const target = Math.max(
        0,
        Math.min(maxScroll(touch.node, touch.axis), position + velocity / 15),
      );
      start(touch.node, touch.axis, position, target, velocity);
    }
    touch = null;
  };
  const cancel = () => {
    touch = null;
    stop();
  };
  const cancelForPointer = (event: PointerEvent) => {
    if (event.pointerType !== 'touch') cancel();
  };
  doc.addEventListener('wheel', wheel, { passive: false });
  doc.addEventListener('touchstart', touchStart, { passive: true });
  doc.addEventListener('touchmove', touchMove, { passive: false });
  doc.addEventListener('touchend', touchEnd);
  doc.addEventListener('touchcancel', cancel);
  doc.addEventListener('keydown', cancel);
  doc.addEventListener('pointerdown', cancelForPointer);
  return () => {
    cancel();
    doc.removeEventListener('wheel', wheel);
    doc.removeEventListener('touchstart', touchStart);
    doc.removeEventListener('touchmove', touchMove);
    doc.removeEventListener('touchend', touchEnd);
    doc.removeEventListener('touchcancel', cancel);
    doc.removeEventListener('keydown', cancel);
    doc.removeEventListener('pointerdown', cancelForPointer);
  };
}
