type Status = 'idle' | 'saving' | 'saved' | 'readError' | 'saveError';

/** 一个凭据槽的草稿和串行写入；调用方在更换作用域前等待 flush。 */
export class CredentialDraft {
  private state = { value: '', loaded: false, dirty: false, status: 'idle' as Status };
  private listeners = new Set<() => void>();
  private revision = 0;
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private queue: Promise<unknown> = Promise.resolve();
  private pending: { revision: number; promise: Promise<boolean> } | undefined;
  constructor(private read: () => Promise<string | null>, private write: (value: string) => Promise<void>) {}
  snapshot = () => this.state;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private update(next: Partial<typeof this.state>) {
    this.state = { ...this.state, ...next };
    this.listeners.forEach(listener => listener());
  }
  async load() {
    const generation = ++this.generation;
    const revision = this.revision;
    try {
      const value = await this.read();
      if (generation === this.generation && revision === this.revision) this.update({ value: value ?? '', loaded: true });
    } catch {
      if (generation === this.generation) this.update({ status: 'readError' });
    }
  }
  edit(value: string) {
    if (!this.state.loaded) return;
    clearTimeout(this.timer);
    this.revision += 1;
    this.update({ value, dirty: true, status: 'idle' });
  }
  schedule() {
    clearTimeout(this.timer);
    const revision = this.revision;
    this.timer = setTimeout(() => { if (revision === this.revision) void this.flush(); }, 300);
  }
  pause() { clearTimeout(this.timer); }
  flush = (): Promise<boolean> => {
    clearTimeout(this.timer);
    if (!this.state.dirty) return Promise.resolve(true);
    if (!this.state.loaded) return Promise.resolve(false);
    const revision = this.revision;
    if (this.pending?.revision === revision) return this.pending.promise;
    const generation = this.generation;
    const value = this.state.value;
    this.update({ status: 'saving' });
    const write = this.queue.catch(() => undefined).then(() => this.write(value));
    this.queue = write;
    const promise = write.then(() => {
      if (revision !== this.revision || generation !== this.generation) return false;
      this.update({ dirty: false, status: 'saved' });
      return true;
    }, () => {
      if (revision === this.revision && generation === this.generation) this.update({ status: 'saveError' });
      return false;
    }).finally(() => { if (this.pending?.revision === revision) this.pending = undefined; });
    this.pending = { revision, promise };
    return promise;
  };
  dispose() { this.generation += 1; clearTimeout(this.timer); }
}
