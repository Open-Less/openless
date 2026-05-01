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

export function getCapsuleHostMetrics(
  os: OS,
  translationActive: boolean,
): CapsuleHostMetrics {
  if (os === 'win') {
    return { width: 220, height: translationActive ? 118 : 84, bottomInset: 12, badgeGap: 8 };
  }

  return { width: 176, height: translationActive ? 110 : 42, bottomInset: 0, badgeGap: 8 };
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
