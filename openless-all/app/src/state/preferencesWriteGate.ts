export interface IncomingPreference<T> {
  value: T;
  wasPending: boolean;
  isOwnWrite: boolean;
}

/** Correlates preference broadcasts with writes initiated by this webview. */
export class PreferencesWriteGate<T = unknown> {
  private readonly pendingWrites = new Map<number, T>();
  private readonly recentWrites: T[] = [];
  private nextWriteId = 0;

  constructor(
    private readonly equals: (left: T, right: T) => boolean = Object.is,
  ) {}

  beginWrite(value: T): (canonical?: T) => boolean {
    const writeId = this.nextWriteId++;
    this.pendingWrites.set(writeId, value);
    let finished = false;
    return (canonical) => {
      if (finished) return false;
      finished = true;
      this.pendingWrites.delete(writeId);
      this.remember(value);
      if (canonical !== undefined) this.remember(canonical);
      return this.pendingWrites.size === 0;
    };
  }

  shouldApplyIncoming(): boolean {
    return this.pendingWrites.size === 0;
  }

  receiveIncoming(value: T): IncomingPreference<T> {
    const wasPending = this.pendingWrites.size > 0;
    const isOwnWrite = [
      ...this.pendingWrites.values(),
      ...this.recentWrites,
    ].some(expected => this.equals(expected, value));
    return { value, wasPending, isOwnWrite };
  }

  private remember(value: T) {
    const previousIndex = this.recentWrites.findIndex(item => this.equals(item, value));
    if (previousIndex >= 0) this.recentWrites.splice(previousIndex, 1);
    this.recentWrites.push(value);
    if (this.recentWrites.length > 16) this.recentWrites.shift();
  }
}
