import type { PasteShortcut } from './types';

/**
 * Plain-text insertion mode (issue #994): on Windows, the simulated paste key is
 * Shift+Insert instead of Ctrl+V. Shift+Insert has no letter key, so keyboard-layout
 * switchers (e.g. Punto Switcher) cannot turn it into a stray character.
 */
export const PLAIN_TEXT_PASTE_SHORTCUT: PasteShortcut = 'shiftInsert';
export const DEFAULT_PASTE_SHORTCUT: PasteShortcut = 'ctrlV';

/** Whether the plain-text toggle should render as "on" for the current platform and preference. */
export function isPlainTextInsertionActive(
  isWindows: boolean,
  pasteShortcut: PasteShortcut,
): boolean {
  return isWindows && pasteShortcut === PLAIN_TEXT_PASTE_SHORTCUT;
}

/** Paste shortcut to persist when the plain-text toggle is switched. */
export function pasteShortcutForPlainTextToggle(on: boolean): PasteShortcut {
  return on ? PLAIN_TEXT_PASTE_SHORTCUT : DEFAULT_PASTE_SHORTCUT;
}
