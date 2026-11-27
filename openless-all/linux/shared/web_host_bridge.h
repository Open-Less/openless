#pragma once

// Qt and GTK use the same HTTP bridge and native-only clipboard/open/input channel.
// The host sets __openlessHost and __openlessEngine before this document-start script.
inline constexpr char kOpenlessWebHostScript[] = R"OPENLESS(
(() => {
  window.__openlessSetInputRegions = (rects) =>
    alert('openless-input-regions:' + JSON.stringify(rects));
  window.__openlessCopyText = (text) => {
    alert('openless-copy:' + String(text));
    // Preserve the input method's clipboard history as a best-effort side effect.
    fetch('/api/invoke', { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command: 'copy_text_to_clipboard', args: { text: String(text) } })
    }).catch(() => {});
    return Promise.resolve();
  };
  try {
    const clipboard = navigator.clipboard || {};
    if (!navigator.clipboard)
      Object.defineProperty(Navigator.prototype, 'clipboard', { value: clipboard, configurable: true });
    clipboard.writeText = window.__openlessCopyText;
  } catch (error) { console.warn('openless clipboard shim:', error); }
  window.open = (url) => {
    if (url) alert('openless-open:' + String(url));
    return null;
  };
  window.addEventListener('openless:sync-ui-persistence-failed', (event) =>
    console.error('openless-sync-ui-failed:', event.detail?.message || event.detail?.code));
  window.addEventListener('unhandledrejection', (event) =>
    console.error('openless-unhandled-rejection:', event.reason?.message || String(event.reason)));
  window.addEventListener('error', (event) => console.error('openless-window-error:', event.message));
  const markEngine = () => {
    document.documentElement.dataset.olEngine = window.__openlessEngine;
  };
  if (document.documentElement) markEngine();
  else document.addEventListener('DOMContentLoaded', markEngine, { once: true });
})();
)OPENLESS";
