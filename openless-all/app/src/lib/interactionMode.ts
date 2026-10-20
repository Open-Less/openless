let keyboard = false;
export function keyboardInteraction() {
  return keyboard;
}
export function initInteractionMode() {
  const set = (value: boolean) => {
    keyboard = value;
    document.documentElement.dataset.olInput = value ? 'keyboard' : 'pointer';
  };
  document.addEventListener('keydown', () => set(true), { capture: true, passive: true });
  document.addEventListener('pointerdown', () => set(false), { capture: true, passive: true });
}
