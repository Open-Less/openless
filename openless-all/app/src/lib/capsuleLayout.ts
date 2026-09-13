import type { OS } from '../components/WindowChrome';
import type { CapsuleStyle } from './types';

export type CapsuleMessageKind = 'default' | 'processing' | 'error';

export interface CapsulePillMetrics {
  width: number;
  height: number;
  textWidth: number;
  boxSizing: 'border-box' | 'content-box';
}

export interface CapsuleHostMetrics {
  width: number;
  height: number;
  horizontalInset: number;
  bottomInset: number;
  badgeGap: number;
  boxSizing: 'border-box' | 'content-box';
}

export interface CapsuleMessageLayout {
  allowWrap: boolean;
  lineClamp: number;
}

// 纯光效舞台按 siri-glsl demo 的原始比例呈现：光条横贯 ~420px（demo 画布宽），
// 舞台 460×180 给发光扩散留余量。与 src-tauri/src/lib.rs 的 capsule_window_bounds /
// capsule_visual_height 保持一致。
const VOICE_ORB_STAGE_WIDTH = 460;
const VOICE_ORB_STAGE_HEIGHT = 180;
const VOICE_ORB_TEXT_WIDTH = 400;

// typeless 窗口面积是原尺寸（460×128）的 1/5；内容由 CapsuleStyles.css 的 zoom 缩放，
// 与 src-tauri/src/lib.rs 的 capsule_window_bounds_for_style 保持一致。
const TYPELESS_STAGE_WIDTH = 206;
const TYPELESS_STAGE_HEIGHT = 57;

export function parseCapsuleStyle(value: unknown): CapsuleStyle | undefined {
  return value === 'siri' || value === 'classic' || value === 'typeless' ? value : undefined;
}

export function getCapsulePillMetrics(os: OS): CapsulePillMetrics {
  void os;
  return {
    width: VOICE_ORB_STAGE_WIDTH,
    height: VOICE_ORB_STAGE_HEIGHT,
    textWidth: VOICE_ORB_TEXT_WIDTH,
    boxSizing: 'border-box',
  };
}

export function getCapsuleHostMetrics(
  os: OS,
  translationActive: boolean,
  style: CapsuleStyle = 'siri',
): CapsuleHostMetrics {
  void translationActive;
  if (style === 'typeless') {
    return {
      width: TYPELESS_STAGE_WIDTH,
      height: TYPELESS_STAGE_HEIGHT,
      horizontalInset: 0,
      bottomInset: 0,
      badgeGap: 8,
      boxSizing: 'border-box',
    };
  }
  const stage = getCapsulePillMetrics(os);
  return {
    width: stage.width,
    height: style === 'siri' ? stage.height : style === 'classic' ? 100 : 128,
    horizontalInset: 0,
    bottomInset: style === 'siri' ? 0 : 16,
    badgeGap: 8,
    boxSizing: 'border-box',
  };
}

export function getCapsuleMessageLayout(os: OS, kind: CapsuleMessageKind): CapsuleMessageLayout {
  if (os === 'win' && (kind === 'error' || kind === 'processing')) {
    return { allowWrap: true, lineClamp: 2 };
  }

  return { allowWrap: false, lineClamp: 1 };
}
