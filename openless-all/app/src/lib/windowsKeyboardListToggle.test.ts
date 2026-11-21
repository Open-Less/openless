import {
  effectiveWindowsInsertionMode,
  showWindowsSendInputNewlineMode,
} from './windowsKeyboardListToggle';

function assert(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

assert(
  effectiveWindowsInsertionMode(undefined, true) === 'sendInput',
  'legacy true should resolve to sendInput',
);
assert(
  effectiveWindowsInsertionMode(undefined, false) === 'tsf',
  'legacy false should resolve to tsf',
);
assert(
  effectiveWindowsInsertionMode(undefined, undefined) === 'tsf',
  'missing mode and legacy should resolve to tsf',
);

assert(
  !showWindowsSendInputNewlineMode('tsf', true),
  'explicit tsf must win over legacy true for newline mode',
);

assert(
  !showWindowsSendInputNewlineMode('paste', true),
  'explicit paste should hide newline mode even with legacy true',
);

assert(showWindowsSendInputNewlineMode('sendInput'), 'sendInput should show newline mode');
assert(!showWindowsSendInputNewlineMode('paste'), 'paste should hide newline mode');
assert(!showWindowsSendInputNewlineMode('tsf'), 'tsf should hide newline mode');
assert(
  showWindowsSendInputNewlineMode(undefined, true),
  'legacy sendInput-only should show newline mode',
);

console.log('windowsKeyboardListToggle.test.ts: ok');
