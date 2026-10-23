// Small synchronous, same-origin bootstrap. Allowed by the native script-src 'self' CSP.
performance.mark('openless:html');
try {
  const theme = localStorage.getItem('ol.theme');
  if (theme === 'dark') document.documentElement.dataset.olTheme = 'dark';
  if (theme === 'light') document.documentElement.dataset.olInitialLight = '';
} catch {
  /* Storage can be disabled; the OS color scheme is the fallback. */
}
if (new URLSearchParams(location.search).has('window')) {
  document.documentElement.dataset.olAux = '';
} else {
  // Match fontScale.ts before the first HTML frame, not only after React loads.
  let scale = 1.1;
  try {
    const saved = localStorage.getItem('ol-font-scale');
    if (saved === 'small') scale = 0.9;
    if (saved === 'medium') scale = 1;
    if (saved === 'large') scale = 1.1;
  } catch {
    /* Keep the shared 110% default when storage is unavailable. */
  }
  document.documentElement.style.zoom = String(scale);
  document.documentElement.style.setProperty('--ol-interface-scale', String(scale));
}
