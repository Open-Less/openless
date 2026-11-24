export const STARTUP_CREDENTIAL_TIMEOUT_MS = 1500;

/** Bound optional startup reads without weakening the backend contract handshake.
 * The race also observes late rejections after a timeout or effect cancellation. */
export async function withStartupDeadline<T>(
  probe: Promise<T>,
  signal: AbortSignal,
  timeoutMs = STARTUP_CREDENTIAL_TIMEOUT_MS,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  const deadline = new Promise<never>((_, reject) => {
    abort = () => reject(new Error('Startup probe cancelled'));
    if (signal.aborted) {
      abort();
      return;
    }
    signal.addEventListener('abort', abort, { once: true });
    timer = setTimeout(() => reject(new Error('Startup credential probe timed out')), timeoutMs);
  });
  try {
    return await Promise.race([deadline, probe]);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener('abort', abort);
  }
}
