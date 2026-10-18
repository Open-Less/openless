const animations = new WeakMap<Element, Animation>();
/** Visual-only propagation; never calls another switch's handler or changes checked state. */
export function toggleGroupFeedback(source: HTMLElement) {
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  const group = source.closest('[data-ol-toggle-group], [role="group"]');
  if (!group) return;
  const switches = [...group.querySelectorAll<HTMLElement>('[role="switch"]:not([disabled])')];
  const index = switches.indexOf(source);
  switches.forEach((element, i) => {
    const distance = Math.abs(index - i);
    if (!distance || distance > 3 || !element.animate) return;
    const from = getComputedStyle(element).transform;
    animations.get(element)?.cancel();
    const direction = i > index ? 1 : -1;
    const animation = element.animate(
      [
        { transform: from },
        { transform: `translateY(${direction * 1.5}px)`, offset: 0.3 },
        { transform: 'none' },
      ],
      { duration: 240, delay: distance * 25, easing: 'cubic-bezier(0.23, 1, 0.32, 1)' },
    );
    animations.set(element, animation);
    void animation.finished
      .then(() => {
        if (animations.get(element) === animation) animations.delete(element);
        animation.cancel();
      })
      .catch(() => {});
  });
}
