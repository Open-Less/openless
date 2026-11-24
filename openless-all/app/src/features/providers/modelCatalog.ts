type CatalogState = {
  version: number;
  models: string[];
  status: 'loading' | 'success' | 'empty' | 'error';
  error?: unknown;
};

/** Catalog responses only update the candidates; the config version and request sequence
    together decide whether a response is valid. */
export class ModelCatalog {
  private state: CatalogState | null = null;
  private request = 0;
  private listeners = new Set<() => void>();
  snapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(state: CatalogState) {
    this.state = state;
    this.listeners.forEach((listener) => listener());
  }
  invalidate() {
    this.request += 1;
  }
  async load(
    version: number,
    read: () => Promise<{ models: string[] }>,
    currentVersion: () => number,
  ) {
    const request = ++this.request;
    const current = () => request === this.request && version === currentVersion();
    this.update({ version, models: [], status: 'loading' });
    try {
      const result = await read();
      if (current())
        this.update({
          version,
          models: result.models,
          status: result.models.length ? 'success' : 'empty',
        });
    } catch (error) {
      if (current()) this.update({ version, models: [], status: 'error', error });
    }
  }
}
