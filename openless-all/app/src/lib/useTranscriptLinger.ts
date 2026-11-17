import { useEffect, useState } from 'react';

interface LingerTrack {
  shown: string;
  visible: boolean;
  closing: boolean;
}

/**
 * Siri enhanced close: when the band stops being visible (Done, Cancel, Error), keep it and its
 * last line for `ms` so it can collapse into the wave instead of vanishing. `visible` is separate
 * from `text` because the enhanced band shows as soon as recording starts, before any words.
 * Derived during render, so the band never drops out for a frame before the closing pass starts.
 */
export function useTranscriptLinger(
  text: string,
  visible: boolean,
  active: boolean,
  ms: number,
): { text: string; visible: boolean; closing: boolean } {
  const [track, setTrack] = useState<LingerTrack>({ shown: text, visible, closing: false });
  let next = track;
  if (visible) {
    if (track.shown !== text || !track.visible || track.closing) {
      next = { shown: text, visible: true, closing: false };
    }
  } else if (track.visible && !track.closing) {
    next = active
      ? { shown: track.shown, visible: true, closing: true }
      : { shown: '', visible: false, closing: false };
  }
  if (next !== track) setTrack(next);

  useEffect(() => {
    if (!track.closing) return undefined;
    const timer = window.setTimeout(
      () =>
        setTrack((current) =>
          current.closing ? { shown: '', visible: false, closing: false } : current,
        ),
      ms,
    );
    return () => window.clearTimeout(timer);
  }, [track.closing, ms]);

  return next.closing
    ? { text: next.shown, visible: true, closing: true }
    : { text, visible, closing: false };
}
