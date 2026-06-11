/** Android-specific preference and status types (mirrors Rust IPC payloads). */

export type AndroidInsertStrategy = 'accessibility' | 'clipboard';
export type AndroidOverlayTrigger = 'background' | 'keyboard' | 'always';
export type AndroidOverlayActivationMode = 'tap' | 'long_press';
export type AndroidOverlayLeftSwipeAction = 'translation' | 'style_pack';
export type AndroidOverlayCancelSwipeDirection = 'up' | 'down';

export interface AndroidOverlayStatus {
  permission: 'granted' | 'notGranted' | 'notAndroid';
  overlayVisible: boolean;
  message: string;
}

export interface AndroidAccessibilityStatus {
  state: 'enabled' | 'notEnabled' | 'notAndroid';
  enabled: boolean;
  message: string;
}

export type AndroidPreferenceKey =
  | 'androidInsertStrategy'
  | 'androidOverlayTrigger'
  | 'androidOverlayActivationMode'
  | 'androidOverlayLeftSwipeAction'
  | 'androidOverlayCancelSwipeDirection'
  | 'androidOverlaySizeDp';

export function normalizeAndroidOverlayTrigger(
  trigger: AndroidOverlayTrigger,
): AndroidOverlayTrigger {
  return trigger === 'keyboard' ? 'background' : trigger;
}

export function clampAndroidOverlaySize(size: number): number {
  if (!Number.isFinite(size)) return 72;
  return Math.min(120, Math.max(48, Math.round(size / 4) * 4));
}
