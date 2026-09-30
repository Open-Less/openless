import { useEffect, useRef, useState } from 'react';

/**
 * Returns `value`, updated at most once per animation frame.
 *
 * For streaming text (LLM tokens) and similar inputs arriving far faster than the refresh
 * rate: collapses "one re-render per token" into "at most one per frame", so expensive
 * derived computation (full markdown parsing, DOM measurement) runs at the frame rate
 * (~60fps) instead of the token rate. Otherwise parsing a long reply is O(n²) (each token
 * re-parses the entire accumulated text).
 *
 * This is throttle, not debounce: while streaming continues, each frame flushes the latest
 * value; the final value is always delivered (a trailing frame after the stream stops), so
 * no content is lost.
 */
export function useRafThrottle<T>(value: T): T {
  const [throttled, setThrottled] = useState<T>(value);
  const latest = useRef<T>(value);
  const frame = useRef<number | null>(null);
  latest.current = value;

  useEffect(() => {
    // A frame is already scheduled: don't reorder or cancel; the latest value flushes when it fires.
    if (frame.current != null) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      setThrottled(latest.current);
    });
  }, [value]);

  // Cancel a pending frame only on unmount, avoiding setState on an unmounted component.
  useEffect(
    () => () => {
      if (frame.current != null) cancelAnimationFrame(frame.current);
    },
    [],
  );

  return throttled;
}
