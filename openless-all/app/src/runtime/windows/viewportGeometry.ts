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
    let remeasure: ReturnType<typeof setInterval> | undefined;
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
      clearInterval(remeasure);
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
    // A WebView resize may settle without a ResizeObserver delivery. Poll only
    // while this geometry request is pending; the observed size remains authoritative.
    remeasure = setInterval(check, 64);
    timer = setTimeout(
      () => settle(undefined, new Error('The requested WebView size was not observed')),
      timeoutMs,
    );
    // The resize can be delivered before its native acknowledgement.
    check();
  });
}
