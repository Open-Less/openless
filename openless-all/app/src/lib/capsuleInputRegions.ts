export interface CapsuleInputRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Menus own their scrollable viewport; their offscreen option rows own no extra windows. */
export function collectCapsuleInputRegions(root: Document): CapsuleInputRect[] {
  const view = root.defaultView;
  const width = view?.innerWidth ?? root.documentElement.clientWidth;
  const height = view?.innerHeight ?? root.documentElement.clientHeight;
  const selector =
    'button:not(:disabled), input, select, textarea, a[href], [role="button"], [role="menuitem"], [role="listbox"], [role="menu"]';
  return Array.from(root.querySelectorAll<HTMLElement>(selector)).flatMap((node) => {
    if (
      !node.getClientRects().length ||
      node.matches(':disabled, [aria-disabled="true"]') ||
      node.closest('[aria-hidden="true"], [hidden], [inert]')
    )
      return [];
    const style = view?.getComputedStyle(node);
    if (style?.visibility === 'hidden' || style?.pointerEvents === 'none') return [];
    if (node.parentElement?.closest('[role="listbox"], [role="menu"]')) return [];
    const r = node.getBoundingClientRect();
    const left = Math.max(0, r.left);
    const top = Math.max(0, r.top);
    const right = Math.min(width, r.right);
    const bottom = Math.min(height, r.bottom);
    return right > left && bottom > top
      ? [{ left, top, width: right - left, height: bottom - top }]
      : [];
  });
}
