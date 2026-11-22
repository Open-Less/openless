import { useEffect, useRef, useState } from 'react';
import { qaWindowSetExpanded } from '../../api/qa';
import { useAppViewport } from '../../ui/layout/AppViewport';
import { waitForViewportGeometry, type ViewportSize } from '../../runtime/windows/viewportGeometry';

interface LayoutKey {
  expanded: boolean;
  enterEpoch: number;
}

interface LayoutResult extends LayoutKey {
  ready: boolean;
  failed: boolean;
}

const COMPACT_HEIGHT = 80;

function fitsQa(size: ViewportSize, expanded: boolean) {
  if (size.width < 220) return false;
  // The host clamps dimensions to the monitor work area, so check a usable
  // content region instead of requiring the full window dimensions.
  return expanded
    ? size.height >= COMPACT_HEIGHT + 44
    : size.height >= 55 && size.height <= COMPACT_HEIGHT + 2;
}

export function useQaWindowLayout({
  expanded,
  enterEpoch,
  blocked,
  nativeWindow,
}: LayoutKey & { blocked: boolean; nativeWindow: boolean }) {
  const { boundary } = useAppViewport();
  const [outcome, setOutcome] = useState<LayoutResult | null>(null);
  const writes = useRef<Promise<unknown>>(Promise.resolve());
  const pending = useRef<LayoutKey & { promise: Promise<ViewportSize> }>();

  useEffect(() => {
    if (!nativeWindow || blocked) return;
    const request = new AbortController();
    const viewport = boundary ?? document.documentElement;
    const resize = writes.current
      .catch(() => undefined)
      .then(async () => {
        if (!request.signal.aborted) await qaWindowSetExpanded(expanded);
      });
    writes.current = resize;
    const promise = resize.then(() =>
      waitForViewportGeometry(viewport, (size) => fitsQa(size, expanded), request.signal),
    );
    pending.current = { expanded, enterEpoch, promise };
    void promise.then(
      () => {
        if (!request.signal.aborted)
          setOutcome({ expanded, enterEpoch, ready: true, failed: false });
      },
      () => {
        if (!request.signal.aborted)
          setOutcome({ expanded, enterEpoch, ready: false, failed: true });
      },
    );
    return () => request.abort();
  }, [blocked, boundary, enterEpoch, expanded, nativeWindow]);

  const current = outcome?.expanded === expanded && outcome.enterEpoch === enterEpoch;
  return {
    ready: !nativeWindow || !!(current && outcome.ready),
    failed: nativeWindow && !!(current && outcome.failed),
    waitForReady() {
      if (!nativeWindow) return Promise.resolve();
      const request = pending.current;
      if (request?.expanded !== expanded || request.enterEpoch !== enterEpoch)
        return Promise.reject(new Error('The active window layout is not ready'));
      return request.promise;
    },
  };
}
