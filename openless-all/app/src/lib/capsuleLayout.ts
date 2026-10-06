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

// The pure light-effect stage renders at the siri-glsl demo's original proportions: the light
// bar spans ~420px (demo canvas width), the stage is 460×180 leaving room for glow diffusion.
// Kept in sync with capsule_window_bounds / capsule_visual_height in src-tauri/src/lib.rs.
const VOICE_ORB_STAGE_WIDTH = 460;
const VOICE_ORB_STAGE_HEIGHT = 180;
const VOICE_ORB_TEXT_WIDTH = 400;
export const CAPSULE_TRANSCRIPT_RAIL_HEIGHT = 40;
export const CAPSULE_TRANSCRIPT_RAIL_GAP = 8;
export const CAPSULE_TRANSCRIPT_RAIL_HEIGHTS: Record<CapsuleStyle, number> = {
  siri: 40,
  classic: 52,
  // Typeless is rendered in its 0.447 zoomed tree, but its CSS rail still owns
  // the same 52px logical row as the standalone Windows overlay.
  typeless: 52,
};

export const CLASSIC_CAPSULE_HOST_HEIGHT = 172;
export const CLASSIC_CAPSULE_BOTTOM_INSET = 16;
export const CLASSIC_CAPSULE_BADGE_HEIGHT = 22;
export const CLASSIC_CAPSULE_BADGE_GAP = 8;
export const CLASSIC_CAPSULE_RAIL_GAP = 8;

export interface ClassicCapsuleGeometry {
  hostHeight: number;
  bodyTop: number;
  bodyBottom: number;
  badgeTop: number;
  badgeBottom: number;
  badgeBottomOffset: number;
  railTop: number | null;
  railHeight: number;
}

/**
 * Classic uses one 172px layout coordinate system on desktop. The body, badge
 * lane, and a sibling rail each get an explicit vertical anchor so an external
 * rail is not treated as a missing same-window rail.
 */
export function getClassicCapsuleGeometry(
  os: OS,
  transcriptVisible: boolean,
  translationActive: boolean,
  transcriptInSameWindow: boolean,
): ClassicCapsuleGeometry {
  const pillHeight = os === 'win' ? 52 : 42;
  const hostHeight = CLASSIC_CAPSULE_HOST_HEIGHT;
  const bodyBottom = hostHeight - CLASSIC_CAPSULE_BOTTOM_INSET;
  const bodyTop = bodyBottom - pillHeight;
  const railHeight = getCapsuleTranscriptRailHeight('classic');
  const sameWindowRail = transcriptVisible && transcriptInSameWindow;
  const externalBadgeLane = transcriptVisible && !transcriptInSameWindow && translationActive;
  const railTop = transcriptVisible
    ? bodyTop -
      CLASSIC_CAPSULE_RAIL_GAP -
      railHeight -
      (externalBadgeLane ? CLASSIC_CAPSULE_BADGE_HEIGHT + CLASSIC_CAPSULE_BADGE_GAP : 0)
    : null;
  const badgeBottom =
    bodyTop -
    CLASSIC_CAPSULE_BADGE_GAP -
    (sameWindowRail && translationActive ? railHeight + CLASSIC_CAPSULE_RAIL_GAP : 0);
  const badgeTop = badgeBottom - CLASSIC_CAPSULE_BADGE_HEIGHT;
  return {
    hostHeight,
    bodyTop,
    bodyBottom,
    badgeTop,
    badgeBottom,
    badgeBottomOffset: hostHeight - badgeBottom,
    railTop,
    railHeight,
  };
}

export function getCapsuleTranscriptRailHeight(style: CapsuleStyle): number {
  return CAPSULE_TRANSCRIPT_RAIL_HEIGHTS[style];
}

/** Classic's translating badge lives above the body and must also clear the
 * external 52px rail when the rail is hosted in a sibling HWND. */
export function getClassicTranslationBadgeOffset(
  transcriptVisible: boolean,
  transcriptInSameWindow = true,
): number {
  const geometry = getClassicCapsuleGeometry('win', transcriptVisible, true, transcriptInSameWindow);
  const baseBadgeBottomOffset =
    CLASSIC_CAPSULE_HOST_HEIGHT - geometry.bodyTop + CLASSIC_CAPSULE_BADGE_GAP;
  return geometry.badgeBottomOffset - baseBadgeBottomOffset;
}

export function getClassicProcessingLabel(operating: boolean): 'capsule.thinking' | 'capsule.using' {
  return operating ? 'capsule.using' : 'capsule.thinking';
}

export interface CapsuleTranscriptRailPosition {
  width: number;
  height: number;
  /** Rail top relative to the capsule body HWND top, in logical/native pixels. */
  topOffset: number;
  /** CSS gap between the rail and the capsule's actual body content. */
  gap: number;
}

const TYPELESS_ZOOM = 0.447;
const TYPELESS_CAPSULE_HEIGHT = 64 * TYPELESS_ZOOM;
const TYPELESS_TRANSLATION_ROW_HEIGHT = 20 * TYPELESS_ZOOM;
const TYPELESS_RAIL_HEIGHT = 52 * TYPELESS_ZOOM;

/**
 * Position the standalone Windows rail against the actual capsule body, not
 * against the transparent HWND edge. Keep this in sync with the native helper
 * of the same name in tauri_coordinator_host.rs.
 */
export function getCapsuleTranscriptRailPosition(
  style: CapsuleStyle,
  translationActive: boolean,
  transcriptVisible: boolean,
): CapsuleTranscriptRailPosition | null {
  if (!transcriptVisible) return null;
  if (style === 'siri') {
    const bodyTop = 0;
    const railHeight = 40;
    const gap = 8;
    return { width: 460, height: railHeight, topOffset: bodyTop - gap - railHeight, gap };
  }
  if (style === 'classic') {
    const geometry = getClassicCapsuleGeometry('win', true, translationActive, false);
    const gap = CLASSIC_CAPSULE_RAIL_GAP;
    return {
      width: 460,
      height: geometry.railHeight,
      topOffset: geometry.railTop ?? 0,
      gap,
    };
  }
  const hostHeight = translationActive ? 65 : 57;
  const bodyBottom = hostHeight;
  const bodyTop = bodyBottom - TYPELESS_CAPSULE_HEIGHT - (translationActive ? TYPELESS_TRANSLATION_ROW_HEIGHT : 0);
  const gap = 0;
  return {
    width: 206,
    height: TYPELESS_RAIL_HEIGHT,
    topOffset: bodyTop - gap - TYPELESS_RAIL_HEIGHT,
    gap,
  };
}

const SIRI_HOST_HEIGHT =
  VOICE_ORB_STAGE_HEIGHT + CAPSULE_TRANSCRIPT_RAIL_HEIGHT + CAPSULE_TRANSCRIPT_RAIL_GAP;
const CLASSIC_HOST_HEIGHT = CLASSIC_CAPSULE_HOST_HEIGHT;

// The typeless window is 1/5 the area of the original size (460×128); content is scaled by the
// zoom in CapsuleStyles.css, kept in sync with capsule_window_bounds_for_style in
// src-tauri/src/lib.rs.
const TYPELESS_STAGE_WIDTH = 206;
const TYPELESS_STAGE_HEIGHT = 57;
const TYPELESS_TRANSLATION_HOST_HEIGHT = 65;

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
  if (style === 'typeless') {
    return {
      width: TYPELESS_STAGE_WIDTH,
      height: translationActive ? TYPELESS_TRANSLATION_HOST_HEIGHT : TYPELESS_STAGE_HEIGHT,
      horizontalInset: 0,
      bottomInset: 0,
      badgeGap: 8,
      boxSizing: 'border-box',
    };
  }
  const stage = getCapsulePillMetrics(os);
  return {
    width: stage.width,
    height: style === 'siri' ? SIRI_HOST_HEIGHT : style === 'classic' ? CLASSIC_HOST_HEIGHT : 128,
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
