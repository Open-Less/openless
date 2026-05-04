import type { OS } from '../components/WindowChrome';

export type CapsuleMessageKind = 'default' | 'processing' | 'error';

export interface CapsulePillMetrics {
  width: number;
  height: number;
  textWidth: number;
}

export interface CapsuleHostMetrics {
  width: number;
  height: number;
  bottomInset: number;
  badgeGap: number;
}

export interface CapsuleMessageLayout {
  allowWrap: boolean;
  lineClamp: number;
}

export function getCapsulePillMetrics(os: OS): CapsulePillMetrics {
  if (os === 'win') {
    return { width: 196, height: 52, textWidth: 118 };
  }

  return { width: 176, height: 42, textWidth: 84 };
}

// macOS 走 1.2.11 calc 布局，不依赖 host metrics；Windows 端要更大的 host
// 装下阴影 inset，仍用这一份。
export function getCapsuleHostMetrics(
  os: OS,
  translationActive: boolean,
): CapsuleHostMetrics {
  if (os === 'win') {
    // Windows 不使用阴影，左右刚好容纳胶囊，上下留足空间
    return { width: 212, height: translationActive ? 118 : 84, bottomInset: 16, badgeGap: 8 };
  }
  return { width: 176, height: 42, bottomInset: 0, badgeGap: 8 };
}

export function getCapsuleMessageLayout(
  os: OS,
  kind: CapsuleMessageKind,
): CapsuleMessageLayout {
  if (os === 'win' && (kind === 'error' || kind === 'processing')) {
    return { allowWrap: true, lineClamp: 2 };
  }

  return { allowWrap: false, lineClamp: 1 };
}
