/**
 * Prevents an older `prefs:changed` broadcast from overwriting an optimistic
 * local edit while one or more local writes are queued. Events cannot be
 * correlated to a particular write, so they are discarded instead of replayed
 * after the queue drains; the latest local value is already optimistic.
 */
export class PreferencesWriteGate<T = unknown> {
  private pendingWrites = 0;

  beginWrite(): () => boolean {
    this.pendingWrites += 1;
    let finished = false;
    return () => {
      if (finished) return false;
      finished = true;
      this.pendingWrites = Math.max(0, this.pendingWrites - 1);
      return this.pendingWrites === 0;
    };
  }

  shouldApplyIncoming(): boolean {
    return this.pendingWrites === 0;
  }

  receiveIncoming(value: T): T | null {
    if (this.shouldApplyIncoming()) return value;
    return null;
  }
}
