// Display predicates for the Windows insertion-related settings rows.
// The legacy boolean windowsSendInputInsertionOnly is only a fallback when the new
// windowsInsertionMode field is missing.

import type { WindowsInsertionMode } from './types';

export function effectiveWindowsInsertionMode(
  mode: WindowsInsertionMode | undefined,
  sendInputOnly?: boolean,
): WindowsInsertionMode {
  return mode ?? (sendInputOnly ? 'sendInput' : 'tsf');
}

/** Shows "show OpenLess in the keyboard list" for non-TSF modes (SendInput / Paste). */
export function showWindowsOpenlessKeyboardListToggle(
  mode: WindowsInsertionMode | undefined,
  sendInputOnly?: boolean,
): boolean {
  return effectiveWindowsInsertionMode(mode, sendInputOnly) !== 'tsf';
}

/** Shows the newline-mode option for SendInput only; hidden under Paste. */
export function showWindowsSendInputNewlineMode(
  mode: WindowsInsertionMode | undefined,
  sendInputOnly?: boolean,
): boolean {
  return effectiveWindowsInsertionMode(mode, sendInputOnly) === 'sendInput';
}
