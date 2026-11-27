export class ClipboardEntry {
  id: string = '';
  text: string = '';
  pinned: boolean = false;
  usedAt: number = 0;
}

// App-local text history populated by the active input method's clipboard reader.
export class ClipboardHistory {
  private rows: ClipboardEntry[] = [];

  load(wire: string): void {
    this.rows = [];
    try {
      const saved: ClipboardEntry[] = JSON.parse(wire);
      if (!Array.isArray(saved)) return;
      for (const item of saved) {
        if (!item || typeof item.id !== 'string' || !item.id || typeof item.text !== 'string' ||
          !item.text || item.text.length > 16384 || !Number.isFinite(item.usedAt) ||
          this.rows.some((row: ClipboardEntry) => row.id === item.id || row.text === item.text)) continue;
        const row = new ClipboardEntry();
        row.id = item.id; row.text = item.text; row.pinned = item.pinned === true; row.usedAt = item.usedAt;
        this.rows.push(row);
        if (this.rows.length === 30) break;
      }
      this.sort();
    } catch (_) { this.rows = []; }
  }

  add(text: string): void {
    if (!text || text.length > 16384) return;
    let row = this.rows.find((item: ClipboardEntry) => item.text === text);
    if (!row) {
      if (this.rows.length === 30 && this.rows.every((item: ClipboardEntry) => item.pinned)) return;
      row = new ClipboardEntry();
      row.id = Date.now().toString() + '-' + Math.random().toString(36).slice(2, 10);
      row.text = text;
      this.rows.unshift(row);
    }
    row.usedAt = Date.now();
    this.sort();
    this.rows = this.rows.slice(0, 30);
  }

  private sort(): void {
    this.rows.sort((a: ClipboardEntry, b: ClipboardEntry) => Number(b.pinned) - Number(a.pinned) || b.usedAt - a.usedAt);
  }

  list(): ClipboardEntry[] {
    return this.rows.map((row: ClipboardEntry): ClipboardEntry => {
      const item = new ClipboardEntry();
      item.id = row.id; item.text = row.text; item.pinned = row.pinned; item.usedAt = row.usedAt;
      return item;
    });
  }
  get(id: string): string { return this.rows.find((row: ClipboardEntry) => row.id === id)?.text || ''; }
  pin(id: string): void {
    const row = this.rows.find((item: ClipboardEntry) => item.id === id);
    if (row) { row.pinned = !row.pinned; this.sort(); }
  }
  remove(id: string): void { this.rows = this.rows.filter((row: ClipboardEntry) => row.id !== id); }
  clear(): void { this.rows = []; }
  serialize(): string { return JSON.stringify(this.rows); }
}
