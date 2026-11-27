class SymbolPress {
  context: string;
  symbol: string = '';
  consumed: boolean = false;
  armed: boolean = false;
  mode: string = '';
  x: number = 0;
  y: number = 0;
  moved: boolean = false;
  constructor(context: string) { this.context = context; }
}

// Touch and recognizer callbacks can both finish a press. Consume it once,
// and never carry a held key into another editor, language or layout.
export class SymbolGesture {
  private presses: Map<string, SymbolPress> = new Map();
  begin(key: string, context: string, x: number = 0, y: number = 0): void {
    const press = new SymbolPress(context); press.x = x; press.y = y;
    this.presses.set(key, press);
  }
  moveTouch(key: string, x: number, y: number): boolean {
    const press = this.presses.get(key);
    if (!press) return false;
    if ((x - press.x) * (x - press.x) + (y - press.y) * (y - press.y) <= 100) return false;
    press.moved = true;
    if (press.mode !== 'hold') return false;
    press.armed = false; press.symbol = ''; press.consumed = true;
    return true;
  }
  arm(key: string, symbol: string, context: string, mode: string, swipeEnabled: boolean = true): boolean {
    const press = this.presses.get(key);
    if (!press || press.context !== context || (mode === 'swipe' && !swipeEnabled) || (mode === 'hold' && press.moved)) return false;
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
  finish(key: string, context: string, mode: string, swipeEnabled: boolean = true): string {
    const press = this.presses.get(key);
    if (!press || press.mode !== mode) return '';
    const symbol = press.context === context && press.armed && (mode !== 'swipe' || swipeEnabled) ? press.symbol : '';
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

export function symbolRows(page: number, chinese: boolean): string[][] {
  if (page === 1) return [
    [chinese ? '￥' : '$', '€', '£', '¥', '℃', '°', '×', '÷', '√', 'π'],
    chinese ? ['【', '】', '｛', '｝', '《', '》', '=', '—', '～'] : ['[', ']', '{', '}', '<', '>', '=', '_', '~'],
    chinese ? ['……', '——', '、', '·', '|', '\\', '^'] : ['...', '--', ',', '·', '|', '\\', '^']
  ];
  return chinese ? [
    ['，', '。', '？', '！', '：', '；', '、', '…', '—', '·'],
    ['（', '）', '【', '】', '《', '》', '“', '”', '￥'],
    ['@', '#', '%', '&', '*', '+', '/']
  ] : [
    [',', '.', '?', '!', ':', ';', "'", '"', '-', '_'],
    ['(', ')', '[', ']', '{', '}', '<', '>', '$'],
    ['@', '#', '%', '&', '*', '+', '/']
  ];
}

// Builder event closures can retain their initial label. Resolve punctuation
// against the live language both while rendering and when dispatching a press.
export function localizedSymbol(text: string, chinese: boolean): string {
  const english: string[] = [',', '.', '?', '!', ':', ';', '(', ')', '[', ']', '{', '}', '<', '>', '$', '~', '...', '--', '-', '_', '"'];
  const full: string[] = ['，', '。', '？', '！', '：', '；', '（', '）', '【', '】', '｛', '｝', '《', '》', '￥', '～', '……', '——', '—', '＿', '“'];
  const index = (chinese ? english : full).indexOf(text);
  if (index >= 0) return (chinese ? full : english)[index];
  if (!chinese && text === '”') return '"';
  if (!chinese && text === '…') return '...';
  return text;
}

export function liveSymbolHint(id: string, hint: string, chinese: boolean): string {
  return id.startsWith('digit:') ? digitSymbol(id.slice(6), chinese) : localizedSymbol(hint, chinese);
}
