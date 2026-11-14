/** Keep IME confirmation Enter away from form submission and shortcut handlers. */
export function installImeEnterGuard(doc: Document = document) {
  const composing = new WeakSet<EventTarget>();
  const ended = new WeakMap<EventTarget, number>();
  const editable = (target: EventTarget | null): target is HTMLElement =>
    target instanceof HTMLElement &&
    (target.matches('input, textarea') || target.isContentEditable);
  const start = (event: CompositionEvent) => {
    if (editable(event.target)) composing.add(event.target);
  };
  const end = (event: CompositionEvent) => {
    if (!editable(event.target)) return;
    composing.delete(event.target);
    ended.set(event.target, performance.now());
  };
  const keydown = (event: KeyboardEvent) => {
    if (event.key !== 'Enter' || !editable(event.target)) return;
    const active = composing.has(event.target) || event.isComposing || event.keyCode === 229;
    const recent = performance.now() - (ended.get(event.target) ?? -Infinity) < 500;
    if (!active && !recent) return;
    // The IME must receive its confirmation key as the browser default action.
    if (!active) event.preventDefault();
    event.stopPropagation();
  };
  doc.addEventListener('compositionstart', start, true);
  doc.addEventListener('compositionend', end, true);
  doc.addEventListener('keydown', keydown, true);
  return () => {
    doc.removeEventListener('compositionstart', start, true);
    doc.removeEventListener('compositionend', end, true);
    doc.removeEventListener('keydown', keydown, true);
  };
}
