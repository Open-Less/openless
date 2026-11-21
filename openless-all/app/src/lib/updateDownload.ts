// Retry only a download, never an installation and never a fresh version check.
export async function downloadWithGithubFallback<T>(
  initial: T,
  download: (update: T) => Promise<void>,
  canFallback: (update: T) => boolean,
  fallback: (update: T) => Promise<T>,
): Promise<T> {
  try {
    await download(initial);
    return initial;
  } catch (error) {
    if (!canFallback(initial)) throw error;
    const retry = await fallback(initial);
    await download(retry);
    return retry;
  }
}
