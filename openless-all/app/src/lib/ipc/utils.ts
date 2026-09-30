import { isTauri, invokeOrMock } from './shared';

export { isTauri };

export async function openExternal(url: string): Promise<void> {
  if (!isTauri) {
    window.open(url, '_blank', 'noopener,noreferrer');
    return;
  }
  try {
    const { open } = await import('@tauri-apps/plugin-shell');
    await open(url);
    return;
  } catch (error) {
    console.warn('[external-open] shell plugin failed', error);
  }
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('open_external_url', { url });
    return;
  } catch (error) {
    console.warn('[external-open] native fallback failed', error);
  }
  window.open(url, '_blank', 'noopener,noreferrer');
}

/**
 * Ask the user for a save path and copy the current session log (openless.log) there.
 * Browser dev mode goes through a mock and does not write to disk. Returns the final save
 * absolute path, or null if the user cancelled.
 *
 * Android writes the log to public Downloads through MediaStore.
 */
export async function exportErrorLog(suggestedFileName: string): Promise<string | null> {
  if (!isTauri) {
    return `~/Downloads/${suggestedFileName}`;
  }
  const isAndroid = typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent || '');
  if (isAndroid) {
    return invokeOrMock<string>(
      'export_error_log_to_downloads',
      { fileName: suggestedFileName },
      () => `~/Downloads/${suggestedFileName}`,
    );
  }
  const { save } = await import('@tauri-apps/plugin-dialog');
  const target = await save(
    {
      defaultPath: suggestedFileName,
      filters: [{ name: 'Log', extensions: ['log', 'txt'] }],
    },
  );
  if (!target) return null;
  await invokeOrMock<void>('export_error_log', { targetPath: target }, () => undefined);
  return target;
}

/**
 * Forward key frontend errors (e.g. auto-update install failures) to the Rust file log
 * (openless.log). The webview's console.error does not land in openless.log, so this goes over
 * IPC separately, letting us learn the real cause after the user "exports the log". Never
 * throws — a logging failure must not disturb the caller's error handling.
 */
export async function logClientError(message: string): Promise<void> {
  try {
    await invokeOrMock<void>('log_client_error', { message }, () => undefined);
  } catch (error) {
    console.warn('[log-client-error] failed to forward error to app log', error);
  }
}

/** Probe "the body text around the host app's cursor" once. **Debug only, no product path.**
 *
 *  `delayMs` is what makes this entry point usable: when clicking the button from the settings
 *  page the foreground app is OpenLess itself, so it would always read our own window. A few
 *  seconds of delay gives the user time to switch to Notes / VS Code / WeChat and click into an
 *  input field before the probe actually reads. */
export async function debugReadCursorContext(
  delayMs: number,
): Promise<import('../types').HostDocumentReadResult> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke('debug_read_cursor_context', { delayMs });
}
