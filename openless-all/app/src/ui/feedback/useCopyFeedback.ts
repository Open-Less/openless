import { useEffect, useRef, useState } from 'react';

export function useCopyFeedback(writeText: (text: string) => Promise<void>, onFailure: () => void) {
  const [copied, setCopied] = useState<string | null>(null);
  const revision = useRef(0);
  const timeout = useRef<number>();
  useEffect(
    () => () => {
      revision.current++;
      window.clearTimeout(timeout.current);
    },
    [],
  );

  const copy = async (key: string, text: string) => {
    const request = ++revision.current;
    window.clearTimeout(timeout.current);
    setCopied(null);
    try {
      await writeText(text);
      if (revision.current !== request) return;
      setCopied(key);
      timeout.current = window.setTimeout(() => {
        if (revision.current === request) setCopied(null);
      }, 1500);
    } catch {
      if (revision.current === request) onFailure();
    }
  };
  return { copied, copy };
}
