// Windows 插入相关设置行的显示谓词。
// 旧布尔 windowsSendInputInsertionOnly 只在新字段 windowsInsertionMode 缺失时兜底。

import type { WindowsInsertionMode } from './types';

export function effectiveWindowsInsertionMode(
  mode: WindowsInsertionMode | undefined,
  sendInputOnly?: boolean,
): WindowsInsertionMode {
  return mode ?? (sendInputOnly ? 'sendInput' : 'tsf');
}

/** 非 TSF（SendInput / Paste）时显示「在键盘列表中显示 OpenLess」。 */
export function showWindowsOpenlessKeyboardListToggle(
  mode: WindowsInsertionMode | undefined,
  sendInputOnly?: boolean,
): boolean {
  return effectiveWindowsInsertionMode(mode, sendInputOnly) !== 'tsf';
}

/** 仅 SendInput 时显示换行方式选项；Paste 下隐藏。 */
export function showWindowsSendInputNewlineMode(
  mode: WindowsInsertionMode | undefined,
  sendInputOnly?: boolean,
): boolean {
  return effectiveWindowsInsertionMode(mode, sendInputOnly) === 'sendInput';
}
