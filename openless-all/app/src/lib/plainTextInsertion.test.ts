import { isPlainTextInsertionActive, pasteShortcutForPlainTextToggle } from './plainTextInsertion';

function assert(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

assert(
  pasteShortcutForPlainTextToggle(true) === 'shiftInsert',
  'turning plain-text insertion on should use Shift+Insert',
);
assert(
  pasteShortcutForPlainTextToggle(false) === 'ctrlV',
  'turning plain-text insertion off should restore Ctrl+V',
);

assert(
  isPlainTextInsertionActive(true, 'shiftInsert'),
  'Windows with Shift+Insert should show the toggle as on',
);
assert(
  !isPlainTextInsertionActive(true, 'ctrlV'),
  'Windows with Ctrl+V should show the toggle as off',
);
assert(
  !isPlainTextInsertionActive(true, 'ctrlShiftV'),
  'Windows with Ctrl+Shift+V should show the toggle as off',
);
assert(
  !isPlainTextInsertionActive(false, 'shiftInsert'),
  'non-Windows platforms should never show the Windows plain-text toggle as on',
);
