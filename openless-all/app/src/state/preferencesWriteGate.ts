export interface PreferenceSnapshot<T> {
  preferences: T;
  revision: number;
}
export type PreferenceEdits = Record<string, unknown>;

export function preferenceEdits(previous: unknown, next: unknown, path = ''): PreferenceEdits {
  if (JSON.stringify(previous) === JSON.stringify(next)) return {};
  if (
    previous &&
    next &&
    typeof previous === 'object' &&
    typeof next === 'object' &&
    !Array.isArray(previous) &&
    !Array.isArray(next)
  ) {
    const before = previous as Record<string, unknown>,
      after = next as Record<string, unknown>;
    return Object.assign(
      {},
      ...[...new Set([...Object.keys(before), ...Object.keys(after)])].map((key) =>
        preferenceEdits(
          before[key],
          after[key],
          `${path}/${key.replace(/~/g, '~0').replace(/\//g, '~1')}`,
        ),
      ),
    );
  }
  return { [path]: next ?? null };
}

function applyEdits<T>(value: T, edits: PreferenceEdits): T {
  const result = structuredClone(value);
  for (const [path, value] of Object.entries(edits)) {
    const keys = path
      .slice(1)
      .split('/')
      .map((key) => key.replace(/~1/g, '/').replace(/~0/g, '~'));
    let target = result as Record<string, unknown>;
    let compatible = true;
    for (const key of keys.slice(0, -1)) {
      const child = target[key];
      if (!child || typeof child !== 'object' || Array.isArray(child)) {
        compatible = false;
        break;
      }
      target = child as Record<string, unknown>;
    }
    // A concurrent removal of an optional object wins until the backend rejects this stale edit.
    if (compatible) target[keys[keys.length - 1]] = structuredClone(value);
  }
  return result;
}

/** Backend revisions order snapshots; pending edits belong to individual requests. */
export class PreferencesWriteGate<T> {
  private saved: PreferenceSnapshot<T> | null = null;
  private nextId = 0;
  private pending = new Map<number, PreferenceEdits>();

  receiveIncoming(snapshot: PreferenceSnapshot<T>): T {
    if (!this.saved || snapshot.revision >= this.saved.revision) this.saved = snapshot;
    return this.current();
  }

  beginWrite(previous: T, next: T) {
    const id = ++this.nextId;
    const edits = preferenceEdits(previous, next);
    this.pending.set(id, edits);
    return { id, edits };
  }

  finishWrite(id: number, snapshot?: PreferenceSnapshot<T>): T {
    if (snapshot) this.receiveIncoming(snapshot);
    this.pending.delete(id);
    return this.current();
  }

  current(): T {
    if (!this.saved) throw new Error('preferences not loaded');
    let value = this.saved.preferences;
    for (const edits of this.pending.values()) value = applyEdits(value, edits);
    return value;
  }
}
