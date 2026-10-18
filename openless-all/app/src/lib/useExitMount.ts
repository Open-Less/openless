// Keep overlays mounted until the exit animation finishes after close; reopening cancels the
// pending unmount timer.
// Callers share the exit duration with the overlay motion primitive.

import { useLayoutEffect, useState } from 'react';
import { OVERLAY_EXIT_MS, useReducedMotion } from './motion';

export function useExitMount(open: boolean, exitMs = OVERLAY_EXIT_MS) {
  const [mounted, setMounted] = useState(open);
  const reduced = useReducedMotion();
  // Commit retention before the first visible frame. A fast Escape can otherwise
  // close an overlay before the passive effect has remembered that it opened.
  useLayoutEffect(() => {
    if (open) {
      setMounted(true);
      return;
    }
    if (!mounted) return;
    if (reduced) {
      setMounted(false);
      return;
    }
    const timer = window.setTimeout(() => {
      setMounted(false);
    }, exitMs);
    return () => window.clearTimeout(timer);
  }, [open, mounted, exitMs, reduced]);
  return { mounted: open || mounted, closing: !open };
}
