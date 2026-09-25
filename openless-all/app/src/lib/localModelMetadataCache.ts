/** Coalesce model metadata requests, including callers from different renders. */
export class LocalModelMetadataCache<T> {
  private readonly entries = new Map<string, { promise: Promise<T>; expires: number }>();

  load(key: string, request: () => Promise<T>): Promise<T> {
    const cached = this.entries.get(key);
    if (cached && cached.expires > Date.now()) return cached.promise;
    const promise = Promise.resolve().then(request);
    this.entries.set(key, { promise, expires: Date.now() + 5 * 60_000 });
    void promise.catch(() => {
      if (this.entries.get(key)?.promise === promise) this.entries.delete(key);
    });
    return promise;
  }

  clear(): void {
    this.entries.clear();
  }
}
