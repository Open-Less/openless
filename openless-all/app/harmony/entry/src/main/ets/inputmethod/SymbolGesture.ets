class SymbolPress {
  context: string;
  symbol: string = '';
  consumed: boolean = false;
  armed: boolean = false;
  mode: string = '';
  constructor(context: string) { this.context = context; }
}

// Touch and recognizer callbacks can both finish a press. Consume it once,
// and never carry a held key into another editor, language or layout.
export class SymbolGesture {
  private presses: Map<string, SymbolPress> = new Map();
  begin(key: string, context: string): void { this.presses.set(key, new SymbolPress(context)); }
  arm(key: string, symbol: string, context: string, mode: string): boolean {
    const press = this.presses.get(key);
    if (!press || press.context !== context) return false;
    press.consumed = true;
    press.symbol = symbol;
    press.mode = mode;
    press.armed = symbol.length > 0;
    return press.armed;
  }
  move(key: string, x: number, y: number, distance: number): boolean {
    const press = this.presses.get(key);
    if (!press || !press.consumed || press.mode !== 'swipe') return false;
    press.armed = press.symbol.length > 0 && y <= -distance && Math.abs(x) <= -y * 0.55;
    return press.armed;
  }
  finish(key: string, context: string, mode: string): string {
    const press = this.presses.get(key);
    if (!press || press.mode !== mode) return '';
    const symbol = press.context === context && press.armed ? press.symbol : '';
    press.armed = false;
    press.symbol = '';
    return symbol;
  }
  tap(key: string, context: string): boolean {
    const press = this.presses.get(key);
    if (!press || press.context !== context || press.consumed) return false;
    press.consumed = true;
    return true;
  }
  cancel(key: string, mode: string = ''): boolean {
    const press = this.presses.get(key);
    if (!press || (mode && press.mode !== mode)) return false;
    press.armed = false; press.symbol = ''; press.consumed = true;
    return true;
  }
}

export function letterSymbol(row: number, position: number, split: boolean, chinese: boolean): string {
  const hints: string[][] = chinese ? [
    ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'],
    ['~', '@', '#', '￥', '%', '&', '*', '（', '）'],
    ['，', '。', '？', '！', '：', '；', "'"]
  ] : [
    ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'],
    ['~', '@', '#', '$', '%', '&', '*', '(', ')'],
    [',', '.', '?', '!', ':', ';', "'"]
  ];
  const duplicated = split && ((row === 1 && position >= 5) || (row === 2 && position >= 4));
  if (row < 0 || row >= hints.length) return '';
  return hints[row][duplicated ? position - 1 : position] || '';
}

export function digitSymbol(digit: string, chinese: boolean): string {
  const index = '1234567890'.indexOf(digit);
  const symbols: string[] = chinese ? ['！', '@', '#', '￥', '%', '……', '&', '*', '（', '）'] :
    ['!', '@', '#', '$', '%', '^', '&', '*', '(', ')'];
  return index >= 0 ? symbols[index] : '';
}
