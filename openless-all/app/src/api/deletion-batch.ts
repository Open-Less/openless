export class IncompleteDeletion extends Error {
  constructor(readonly remaining: number) {
    super('Some deletions did not complete');
  }
}

/** Retain the confirmed target set and never replay an acknowledged deletion. */
export function createDeletionBatch(ids: string[], remove: (id: string) => Promise<void>) {
  const remaining = new Set(ids);
  return async (onRemoved: (ids: ReadonlySet<string>) => void | Promise<void>) => {
    const removed = new Set<string>();
    for (const id of remaining) {
      try {
        await remove(id);
        remaining.delete(id);
        removed.add(id);
      } catch {
        // These commands are idempotent by ID. A retry only calls unconfirmed IDs.
      }
    }
    await onRemoved(removed);
    if (remaining.size) throw new IncompleteDeletion(remaining.size);
  };
}
