export interface HotkeyRecorderState {
  pressedCodes: string[];
  draftCodes: string[];
}

export interface HotkeyRecorderUpdate {
  state: HotkeyRecorderState;
  commitCodes: string[] | null;
}

export function createHotkeyRecorderState(): HotkeyRecorderState {
  return {
    pressedCodes: [],
    draftCodes: [],
  };
}

export function updateHotkeyRecorderState(
  state: HotkeyRecorderState,
  code: string,
  pressed: boolean,
): HotkeyRecorderUpdate {
  const active = new Set(state.pressedCodes);
  if (pressed) {
    active.add(code);
  } else {
    active.delete(code);
  }

  const pressedCodes = orderHotkeyCodes([...active]);
  const draftCodes = pressed ? pressedCodes : state.draftCodes;
  const shouldCommit = !pressed && pressedCodes.length === 0 && draftCodes.length > 0;

  return {
    state: shouldCommit ? createHotkeyRecorderState() : { pressedCodes, draftCodes },
    commitCodes: shouldCommit ? draftCodes : null,
  };
}

export function orderHotkeyCodes(codes: string[]): string[] {
  const seen = new Set<string>();
  return codes
    .filter((code) => {
      if (!code || seen.has(code)) return false;
      seen.add(code);
      return true;
    })
    .sort((a, b) => hotkeyCodeRank(a) - hotkeyCodeRank(b));
}

function hotkeyCodeRank(code: string): number {
  const index = HOTKEY_CODE_ORDER.indexOf(code);
  if (index >= 0) return index;
  if (/^Key[A-Z]$/.test(code)) return 100 + code.charCodeAt(3);
  if (/^Digit[0-9]$/.test(code)) return 200 + Number(code.slice(5));
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return 300 + Number(code.slice(1));
  if (/^Numpad[0-9]$/.test(code)) return 400 + Number(code.slice(6));
  return 1000;
}

const HOTKEY_CODE_ORDER = [
  'ControlLeft',
  'ControlRight',
  'AltLeft',
  'AltRight',
  'ShiftLeft',
  'ShiftRight',
  'MetaLeft',
  'MetaRight',
  'Fn',
  'FnLock',
  'CapsLock',
  'ScrollLock',
  'Pause',
  'PrintScreen',
  'Backspace',
  'Tab',
  'Enter',
  'Space',
  'Insert',
  'Delete',
  'Home',
  'End',
  'PageUp',
  'PageDown',
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'ContextMenu',
  'Backquote',
  'Minus',
  'Equal',
  'BracketLeft',
  'BracketRight',
  'Backslash',
  'Semicolon',
  'Quote',
  'Comma',
  'Period',
  'Slash',
  'NumpadAdd',
  'NumpadSubtract',
  'NumpadMultiply',
  'NumpadDivide',
  'NumpadDecimal',
  'NumpadEnter',
  'Mouse4',
  'Mouse5',
];

/** Prefer physical function-key codes; WebKit can expose a private-use key value. */
export function functionKeyPrimaryFromEvent(event: { code: string; key: string }): string | null {
  const supported = /^F([1-9]|1[0-9]|20)$/;
  if (supported.test(event.code)) return event.code;
  if (supported.test(event.key)) return event.key;
  return null;
}

/**
 * Normalize a keyboard event into the ShortcutBinding primary string.
 * Space must use the named code — `e.key === ' '` is length 1 and would otherwise
 * be trimmed to empty by backend validate_primary/parse_primary (#1109).
 */
export function primaryFromKeyboardEvent(event: { code: string; key: string }): string {
  const functionKey = functionKeyPrimaryFromEvent(event);
  if (functionKey) return functionKey;
  const printable = primaryFromPrintableCode(event.code);
  if (printable) return printable;
  if (event.code === 'Space' || event.key === ' ') return 'Space';
  if (event.key.length === 1) return event.key;
  const codeToName: Record<string, string> = {
    Space: 'Space',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
    ArrowUp: 'ArrowUp',
    ArrowDown: 'ArrowDown',
    ArrowLeft: 'ArrowLeft',
    ArrowRight: 'ArrowRight',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
  };
  if (/^F\d{1,2}$/.test(event.key)) return event.key;
  return codeToName[event.code] || event.key;
}

function primaryFromPrintableCode(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  const codeToPrimary: Record<string, string> = {
    Backquote: '`',
    Minus: '-',
    Equal: '=',
    BracketLeft: '[',
    BracketRight: ']',
    Backslash: '\\',
    Semicolon: ';',
    Quote: "'",
    Comma: ',',
    Period: '.',
    Slash: '/',
    IntlBackslash: '\\',
  };
  return codeToPrimary[code] || '';
}
