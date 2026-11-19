/** Keep new content in view only while the reader has retained bottom ownership. */
export function installMessageFollow(root: HTMLElement) {
  const viewport = root.querySelector<HTMLElement>('[data-slot="message-scroller-viewport"]');
  const content = root.querySelector<HTMLElement>('[data-slot="message-scroller-content"]');
  if (!viewport || !content) return () => {};
  let following = true;
  let touchY: number | undefined;
  let frame = 0;
  const distance = () => viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop;
  const scrollAway = () => {
    if (viewport.scrollTop > 1) following = false;
  };
  const wheel = (event: WheelEvent) => {
    if (event.deltaY < 0) scrollAway();
  };
  const touchStart = (event: TouchEvent) => {
    touchY = event.touches[0]?.clientY;
  };
  const touchMove = (event: TouchEvent) => {
    if (event.touches.length !== 1) return;
    const nextY = event.touches[0].clientY;
    if (touchY != null && nextY > touchY) scrollAway();
    touchY = nextY;
  };
  const keydown = (event: KeyboardEvent) => {
    if (['ArrowUp', 'PageUp', 'Home'].includes(event.key) || (event.key === ' ' && event.shiftKey))
      scrollAway();
  };
  const scrolled = () => {
    if (distance() <= 1) following = true;
  };
  const clicked = (event: MouseEvent) => {
    if (
      event.target instanceof Element &&
      event.target.closest('[data-slot="message-scroller-button"]')
    )
      following = true;
  };
  const resize = new ResizeObserver(() => {
    // Consecutive resize deliveries can see the old scroll position before the
    // next paint. A temporary geometry gap is not a reader scrolling away.
    if (!following) return;
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      if (following) viewport.scrollTop = viewport.scrollHeight;
    });
  });
  resize.observe(content);
  resize.observe(viewport);
  viewport.addEventListener('scroll', scrolled, { passive: true });
  viewport.addEventListener('wheel', wheel, { passive: true });
  viewport.addEventListener('touchstart', touchStart, { passive: true });
  viewport.addEventListener('touchmove', touchMove, { passive: true });
  viewport.addEventListener('keydown', keydown);
  root.addEventListener('click', clicked, true);
  return () => {
    resize.disconnect();
    cancelAnimationFrame(frame);
    viewport.removeEventListener('scroll', scrolled);
    viewport.removeEventListener('wheel', wheel);
    viewport.removeEventListener('touchstart', touchStart);
    viewport.removeEventListener('touchmove', touchMove);
    viewport.removeEventListener('keydown', keydown);
    root.removeEventListener('click', clicked, true);
  };
}
