/**
 * 直播转写插入动画的纯函数模型。
 *
 * 坐标直觉（与参考视频一致）：
 * - 新字出现在右侧插入点：淡入 + 上弹回落
 * - 既有文字被挤向左侧，延迟随离插入点的距离增加（声波传播）
 * - 胶囊先以右缘为锚向左拓宽，再整体右移回到居中
 *
 * 这些量只描述「何时、往哪」；真正的弹簧由 LiveTranscriptPill 交给 framer-motion。
 */

export const INSERT_TEXT_MOTION = {
  waveSpeedPxPerMs: 2.35,
  glyphGapPx: 1.4,
  groupDelayMs: 26,
  groupRippleMs: 2,
  bornFadeMs: 180,
  bornY: 9,
  bornBlurPx: 1.8,
  bornStaggerMs: 8,
  widthDelayMinMs: 12,
  recenterExtraMs: 140,
  maxDelayMs: 96,
  charSpring: { stiffness: 460, damping: 23, mass: 0.8 },
  shiftSpring: { stiffness: 260, damping: 30, mass: 1 },
  widthSpring: { stiffness: 330, damping: 32, mass: 0.9 },
  recenterSpring: { stiffness: 180, damping: 27, mass: 1 },
  padX: 18,
  minWidth: 72,
} as const;

export interface InsertUnit {
  key: string;
  text: string;
  born: boolean;
}

let insertUnitSeq = 0;

export function resetInsertUnitKeys() {
  insertUnitSeq = 0;
}

function allocInsertKey() {
  insertUnitSeq += 1;
  return `ins-${insertUnitSeq}`;
}

export function segmentInsertUnits(text: string): string[] {
  const SegmenterCtor = (
    Intl as typeof Intl & {
      Segmenter?: new (
        locales?: string | string[],
        options?: { granularity?: 'grapheme' | 'word' | 'sentence' },
      ) => { segment(input: string): Iterable<{ segment: string }> };
    }
  ).Segmenter;
  if (typeof SegmenterCtor === 'function') {
    return Array.from(
      new SegmenterCtor(undefined, { granularity: 'grapheme' }).segment(text),
      (part) => part.segment,
    );
  }
  return Array.from(text);
}

/**
 * 以最长公共前缀 + 后缀对齐，给稳定字素分配稳定 key。
 * 中间新增的字素 marked born，下一轮会被压成 born: false。
 */
export function diffInsertUnits(prev: InsertUnit[], nextText: string): InsertUnit[] {
  const next = segmentInsertUnits(nextText);
  if (next.length === 0) return [];

  const prevTexts = prev.map((unit) => unit.text);
  let prefix = 0;
  while (prefix < prevTexts.length && prefix < next.length && prevTexts[prefix] === next[prefix]) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < prevTexts.length - prefix &&
    suffix < next.length - prefix &&
    prevTexts[prevTexts.length - 1 - suffix] === next[next.length - 1 - suffix]
  ) {
    suffix += 1;
  }

  const units: InsertUnit[] = [];
  for (let i = 0; i < prefix; i += 1) {
    units.push({ ...prev[i], born: false });
  }
  const middle = next.slice(prefix, next.length - suffix);
  for (const glyph of middle) {
    units.push({ key: allocInsertKey(), text: glyph, born: true });
  }
  if (suffix > 0) {
    const suffixStart = prev.length - suffix;
    for (let i = 0; i < suffix; i += 1) {
      units.push({ ...prev[suffixStart + i], born: false });
    }
  }
  return units;
}

export function firstBornIndex(units: InsertUnit[]): number {
  const index = units.findIndex((unit) => unit.born);
  if (index >= 0) return index;
  return Math.max(0, units.length - 1);
}

export function prefixWidths(widths: number[]): number[] {
  const starts: number[] = [];
  let cursor = 0;
  for (const width of widths) {
    starts.push(cursor);
    cursor += width;
  }
  return starts;
}

export function propagationDelayMs(
  distancePx: number,
  waveSpeedPxPerMs = INSERT_TEXT_MOTION.waveSpeedPxPerMs,
): number {
  if (!Number.isFinite(distancePx) || distancePx <= 0) return 0;
  if (!Number.isFinite(waveSpeedPxPerMs) || waveSpeedPxPerMs <= 0) return 0;
  return Math.min(INSERT_TEXT_MOTION.maxDelayMs, distancePx / waveSpeedPxPerMs);
}

export function planCharDelays(widths: number[], originIndex: number, insertedCount = 1): number[] {
  if (widths.length === 0) return [];
  const starts = prefixWidths(widths);
  const safeOrigin = Math.min(Math.max(0, originIndex), widths.length - 1);
  const originX = starts[safeOrigin] + widths[safeOrigin] / 2;
  // Recognition batches carry the impulse: 2/3/4 inserted glyphs move in
  // corresponding groups. A single-glyph update still pushes a 3-glyph cluster.
  const groupSize = insertedCount <= 1 ? 3 : Math.min(4, insertedCount);
  return widths.map((width, index) => {
    if (index < safeOrigin) {
      const distance = safeOrigin - 1 - index;
      return Math.min(
        INSERT_TEXT_MOTION.maxDelayMs,
        4 +
          Math.floor(distance / groupSize) * INSERT_TEXT_MOTION.groupDelayMs +
          (distance % groupSize) * INSERT_TEXT_MOTION.groupRippleMs,
      );
    }
    const center = starts[index] + width / 2;
    const bornBoost =
      index >= safeOrigin ? (index - safeOrigin) * INSERT_TEXT_MOTION.bornStaggerMs : 0;
    return Math.min(
      INSERT_TEXT_MOTION.maxDelayMs,
      propagationDelayMs(Math.abs(center - originX)) + bornBoost,
    );
  });
}

export interface CapsuleInsertPlan {
  width: number;
  left: number;
  rightEdge: number;
  widthDelayMs: number;
  recenterDelayMs: number;
}

export function planCapsuleInsertMotion(args: {
  stageWidth: number;
  contentWidth: number;
  originX: number;
  minWidth?: number;
  maxWidth?: number;
  padX?: number;
}): CapsuleInsertPlan {
  const padX = args.padX ?? INSERT_TEXT_MOTION.padX;
  const minWidth = args.minWidth ?? INSERT_TEXT_MOTION.minWidth;
  const maxWidth = args.maxWidth ?? Number.POSITIVE_INFINITY;
  const hugged = Math.max(minWidth, args.contentWidth + padX * 2);
  const width = Math.min(maxWidth, hugged);
  const left = (args.stageWidth - width) / 2;
  const rightEdge = left + width;
  const originFromLeft = Math.max(0, Math.min(args.contentWidth, args.originX));
  const widthDelayMs = INSERT_TEXT_MOTION.widthDelayMinMs + propagationDelayMs(originFromLeft);
  const recenterDelayMs = widthDelayMs + INSERT_TEXT_MOTION.recenterExtraMs;
  return { width, left, rightEdge, widthDelayMs, recenterDelayMs };
}

export function clampInsertContentWidth(
  contentWidth: number,
  maxWidth: number,
  padX?: number,
): number {
  const pad = padX ?? INSERT_TEXT_MOTION.padX;
  const inner = Math.max(0, maxWidth - pad * 2);
  return Math.min(contentWidth, inner);
}

/** Right-anchored positions remain independent of the animated shell width. */
export function rightAnchoredPositions(widths: number[]): number[] {
  const total = widths.reduce((sum, width) => sum + width, 0);
  return prefixWidths(widths).map((start) => start - total);
}

/** Only an appended suffix enters beyond the old tail. A correction already has
 * a destination gap and must not sweep across its retained suffix. */
export function appendedBirthAdvance(units: InsertUnit[], widths: number[]): number {
  const origin = units.findIndex((unit) => unit.born);
  if (origin <= 0 || units.slice(origin).some((unit) => !unit.born)) return 0;
  return widths.slice(origin).reduce((sum, width) => sum + width, 0);
}
