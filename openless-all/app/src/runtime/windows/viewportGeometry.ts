export interface ViewportSize {
  width: number;
  height: number;
}

export function readViewportSize(viewport: HTMLElement): ViewportSize {
  return { width: viewport.clientWidth, height: viewport.clientHeight };
}

export function waitForViewportGeometry(
  viewport: HTMLElement,
  fits: (size: ViewportSize) => boolean,
  signal: AbortSignal,
  timeoutMs = 1_400,
): Promise<ViewportSize> {
  return new Promise((resolve, reject) => {
    const aborted = () =>
      settle(undefined, new DOMException('Window layout superseded', 'AbortError'));
    let timer: ReturnType<typeof setTimeout> | undefined;
    let observer: ResizeObserver | undefined;
    const host = viewport.ownerDocument.defaultView;
    const check = () => {
      if (signal.aborted) return aborted();
      const size = readViewportSize(viewport);
      if (viewport.isConnected && fits(size)) settle(size);
    };
    function settle(size?: ViewportSize, error?: Error) {
      signal.removeEventListener('abort', aborted);
      host?.removeEventListener('resize', check);
      host?.visualViewport?.removeEventListener('resize', check);
      observer?.disconnect();
      clearTimeout(timer);
      if (size) resolve(size);
      else reject(error);
    }
    if (signal.aborted) return aborted();
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver(check);
      observer.observe(viewport);
    }
    signal.addEventListener('abort', aborted, { once: true });
    host?.addEventListener('resize', check);
    host?.visualViewport?.addEventListener('resize', check);
    timer = setTimeout(
      () => settle(undefined, new Error('The requested WebView size was not observed')),
      timeoutMs,
    );
    // The resize can be delivered before its native acknowledgement.
    check();
  });
}
