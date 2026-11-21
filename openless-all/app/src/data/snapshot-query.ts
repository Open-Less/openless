export interface QuerySnapshot<T> {
  data: T | null;
  loading: boolean;
  error: unknown;
}

export class SnapshotQuery<T> {
  private snapshot: QuerySnapshot<T>;
  private subscribers = new Set<() => void>();
  private wanted = 0;
  private published = 0;
  private running: Promise<QuerySnapshot<T>> | null = null;

  constructor(
    private readonly read: () => Promise<T>,
    private readonly beforeRead: () => Promise<void>,
    private readonly hydration: { initialData?: T | null; remember?: (data: T) => void } = {},
  ) {
    this.snapshot = { data: hydration.initialData ?? null, loading: true, error: null };
  }

  getSnapshot = (): QuerySnapshot<T> => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.subscribers.add(listener);
    if (this.wanted === 0) void this.refresh();
    return () => {
      this.subscribers.delete(listener);
    };
  };

  load = (): Promise<QuerySnapshot<T>> =>
    this.running ?? (this.wanted === 0 ? this.refresh() : Promise.resolve(this.snapshot));

  refresh = (): Promise<QuerySnapshot<T>> => {
    this.wanted++;
    return this.run();
  };

  invalidate(): void {
    if (this.wanted > 0) void this.refresh();
  }

  private publish(snapshot: QuerySnapshot<T>): void {
    this.snapshot = snapshot;
    for (const subscriber of this.subscribers) subscriber();
  }

  private run(): Promise<QuerySnapshot<T>> {
    this.running ??= Promise.resolve().then(async () => {
      try {
        while (this.published !== this.wanted) {
          if (!this.snapshot.loading) {
            this.publish({ ...this.snapshot, loading: true, error: null });
          }
          let generation = this.wanted;
          try {
            await this.beforeRead();
            generation = this.wanted;
            const data = await this.read();
            // Signals invalidate requests even when their actual response arrives later.
            if (generation !== this.wanted) continue;
            this.published = generation;
            this.hydration.remember?.(data);
            this.publish({ data, loading: false, error: null });
          } catch (error) {
            if (generation !== this.wanted) continue;
            this.published = generation;
            this.publish({ ...this.snapshot, loading: false, error });
          }
        }
        return this.snapshot;
      } finally {
        this.running = null;
      }
    });
    return this.running;
  }
}
