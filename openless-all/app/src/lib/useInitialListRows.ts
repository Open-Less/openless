import { startTransition, useEffect, useState } from 'react';

const INITIAL_LIST_ROWS = 24;

export function useInitialListRows<T>(rows: T[]): T[] {
  const [complete, setComplete] = useState(false);
  const renderRemainder = !complete && rows.length > INITIAL_LIST_ROWS;
  useEffect(() => {
    if (!renderRemainder) return;
    // Paint the visible rows before building the rest of the scrollable list.
    let next = 0;
    const first = requestAnimationFrame(() => {
      next = requestAnimationFrame(() => {
        startTransition(() => setComplete(true));
      });
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(next);
    };
  }, [renderRemainder]);
  return renderRemainder ? rows.slice(0, INITIAL_LIST_ROWS) : rows;
}
