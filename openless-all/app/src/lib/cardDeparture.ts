import { keyboardInteraction } from './interactionMode';
const pending = new WeakSet<HTMLElement>();
/** Small compositor-only arc toward the card's own delete affordance; never blocks Undo. */
export async function cardDeparture(element: HTMLElement | undefined) {
  if (!element || pending.has(element)) return;
  pending.add(element);
  try {
    if (
      keyboardInteraction() ||
      matchMedia('(prefers-reduced-motion: reduce)').matches ||
      !element.animate
    )
      return;
    const transform = getComputedStyle(element).transform;
    const width = element.offsetWidth,
      height = element.offsetHeight;
    const animation = element.animate(
      [
        { transform, opacity: 1, offset: 0 },
        {
          transform: `translate(${width * 0.18}px, ${-height * 0.12}px) rotate(4deg) scale(0.9)`,
          opacity: 0.8,
          offset: 0.45,
        },
        {
          transform: `translate(${width * 0.32}px, ${height * 0.3}px) rotate(10deg) scale(0.65)`,
          opacity: 0,
          offset: 1,
        },
      ],
      { duration: 240, easing: 'cubic-bezier(0.23, 1, 0.32, 1)', fill: 'forwards' },
    );
    await animation.finished.catch(() => {});
    // Retain the invisible endpoint while AnimatePresence removes the node.
    window.setTimeout(() => animation.cancel(), 250);
  } finally {
    pending.delete(element);
  }
}
