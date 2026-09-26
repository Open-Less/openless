import {
  chordModifiersFromPressedCodes,
  formatComboParts,
  genericModifiersFromPressedCodes,
  MODIFIER_CHORD_PRIMARY,
  modifiersFromPressedCodes,
  shortcutFromLegacyTrigger,
  sideModifiersFromPressedCodes,
} from './hotkey';

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(
      `${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  }
}

function assertDeepEqual(actual: string[], expected: string[], message: string) {
  assertEqual(actual.join(','), expected.join(','), message);
}

function mockNavigator(platform: string, userAgent = '') {
  Object.defineProperty(globalThis, 'navigator', {
    value: { platform, userAgent },
    configurable: true,
  });
}

assertEqual(
  shortcutFromLegacyTrigger('leftCommand').primary,
  'LeftCommand',
  'maps leftCommand trigger primary',
);

assertDeepEqual(
  sideModifiersFromPressedCodes(new Set(['MetaLeft', 'ShiftRight'])),
  ['cmd-left', 'shift-right'],
  'MetaLeft+ShiftRight produces side tags',
);

assertDeepEqual(
  modifiersFromPressedCodes(new Set(['MetaLeft', 'ShiftRight']), true),
  ['cmd-left', 'shift-right'],
  'side-specific mode uses side tags',
);

mockNavigator('MacIntel', 'Macintosh');
assertDeepEqual(
  genericModifiersFromPressedCodes(new Set(['MetaLeft', 'ShiftRight'])),
  ['cmd', 'shift'],
  'macOS generic mode maps Meta to cmd',
);

mockNavigator('Win32', 'Windows');
assertDeepEqual(
  genericModifiersFromPressedCodes(new Set(['MetaLeft', 'ShiftRight'])),
  ['super', 'shift'],
  'non-mac generic mode maps Meta to super',
);

assertDeepEqual(
  modifiersFromPressedCodes(new Set(['MetaLeft', 'ShiftRight'])),
  ['super', 'shift'],
  'default recording on non-mac uses super',
);

assertDeepEqual(
  formatComboParts({
    primary: MODIFIER_CHORD_PRIMARY,
    modifiers: ['ctrl-left', 'cmd-left'],
  }),
  ['左 Win', '左 Ctrl'],
  'modifier chord display contains only its physical modifiers',
);

assertDeepEqual(
  sideModifiersFromPressedCodes(new Set(['MetaRight', 'KeyD'])),
  ['cmd-right'],
  'MetaRight+D binding uses cmd-right only',
);

assertDeepEqual(
  sideModifiersFromPressedCodes(new Set(['MetaLeft', 'MetaRight'])),
  ['cmd-left'],
  'left cmd wins when both meta keys are tracked',
);

for (const [left, right, modifier] of [
  ['MetaLeft', 'MetaRight', 'cmd'],
  ['ControlLeft', 'ControlRight', 'ctrl'],
  ['AltLeft', 'AltRight', 'alt'],
  ['ShiftLeft', 'ShiftRight', 'shift'],
]) {
  assertDeepEqual(
    chordModifiersFromPressedCodes(new Set([right, left])),
    [`${modifier}-left`, `${modifier}-right`],
    'modifier-only recording keeps both physical sides independent of press order',
  );
}
assertDeepEqual(
  chordModifiersFromPressedCodes(new Set(['MetaLeft', 'ControlLeft', 'KeyD'])),
  ['cmd-left', 'ctrl-left'],
  'modifier chord ignores ordinary key codes',
);

console.log('hotkeySideModifiers.test.ts passed');
