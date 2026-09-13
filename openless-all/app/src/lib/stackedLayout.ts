// Older preferences may still contain this field. The responsive layout no longer uses it.
export function applyStackedLayoutFromPrefs(_stackedRowLayout?: boolean): void {
  delete document.documentElement.dataset.olStackedLayout;
}
