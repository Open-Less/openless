// Remove the retired layout attribute even when an older saved value is still enabled.
export function applyConservativeLayout(_enabled: boolean): void {
  delete document.documentElement.dataset.olConservativeLayout;
}
