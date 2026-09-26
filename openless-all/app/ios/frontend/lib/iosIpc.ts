// iOS 平台 Tauri IPC 门面。类型与 src-tauri/src/ios/keyboard_status.rs 对齐；
// 在非 iOS 平台调用会返回后端错误字符串，由调用方捕获处理。

import { invoke } from '@tauri-apps/api/core';

export interface IosKeyboardExtensionStatus {
  /** 键盘扩展是否已在系统设置中启用（设置 → 通用 → 键盘 → 键盘）。 */
  enabled: boolean;
  /** 找到的 OpenLess 输入模式 identifier（诊断用）。 */
  identifier: string | null;
}

export function getIosKeyboardStatus(): Promise<IosKeyboardExtensionStatus> {
  return invoke('get_ios_keyboard_status');
}
