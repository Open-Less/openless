/** Keep new content in view only while the reader has retained bottom ownership. */
export function installMessageFollow(root: HTMLElement) {
  const viewport = root.querySelector<HTMLElement>('[data-slot="message-scroller-viewport"]');
  const content = root.querySelector<HTMLElement>('[data-slot="message-scroller-content"]');
  if (!viewport || !content) return () => {};
  let following = true;
  let movedAway = false;
  let previousHeight = viewport.scrollHeight;
  let firstLayout = true;
  let frame = 0;
  const distance = () =>
    Math.max(0, viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop);
  const userScroll = () => {
    following = false;
    movedAway = distance() > 1;
  };
  const keydown = (event: KeyboardEvent) => {
    if (['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' '].includes(event.key))
      userScroll();
  };
  const scrolled = () => {
    if (!following && distance() > 1) movedAway = true;
    if (!following && movedAway && distance() <= 1) following = true;
  };
  const pointerDown = (event: PointerEvent) => {
    if (event.target === viewport) userScroll();
  };
  const clicked = (event: MouseEvent) => {
    if (
      event.target instanceof Element &&
      event.target.closest('[data-slot="message-scroller-button"]')
    )
      following = true;
  };
  const resize = new ResizeObserver(() => {
    const beforeChange = Math.max(0, previousHeight - viewport.clientHeight - viewport.scrollTop);
    previousHeight = viewport.scrollHeight;
    // Content growth can make the previous-height gap negative while the reader
    // is still above the bottom. Only the current geometry can restore follow.
    if (!following && movedAway && distance() <= 1) following = true;
    const shouldFollow = following && (firstLayout || beforeChange <= 40);
    firstLayout = false;
    if (shouldFollow) {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (!following) return;
        viewport.scrollTop = viewport.scrollHeight;
      });
    } else if (beforeChange > 40) {
      following = false;
    }
  });
  resize.observe(content);
  viewport.addEventListener('scroll', scrolled, { passive: true });
  viewport.addEventListener('wheel', userScroll, { passive: true });
  viewport.addEventListener('touchmove', userScroll, { passive: true });
  viewport.addEventListener('keydown', keydown);
  viewport.addEventListener('pointerdown', pointerDown);
  root.addEventListener('click', clicked, true);
  return () => {
    resize.disconnect();
    cancelAnimationFrame(frame);
    viewport.removeEventListener('scroll', scrolled);
    viewport.removeEventListener('wheel', userScroll);
    viewport.removeEventListener('touchmove', userScroll);
    viewport.removeEventListener('keydown', keydown);
    viewport.removeEventListener('pointerdown', pointerDown);
    root.removeEventListener('click', clicked, true);
  };
}
