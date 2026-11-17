type PreviewSignal = 'history' | 'preferences';
const listeners: Record<PreviewSignal, Set<() => void>> = {
  history: new Set(),
  preferences: new Set(),
};

export function publishPreviewChange(domain: PreviewSignal): void {
  for (const listener of listeners[domain]) listener();
}

export function listenPreviewChange(domain: PreviewSignal, listener: () => void): () => void {
  listeners[domain].add(listener);
  return () => {
    listeners[domain].delete(listener);
  };
}
