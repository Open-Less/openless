/**
 * Pure-function model of the live transcription insert animation.
 *
 * Coordinate intuition (matching the reference video):
 * - New characters appear at the right-side insertion point: fade in + pop up and settle
 * - Existing text is pushed left, with delay growing with distance from the insertion point
 *   (sound-wave propagation)
 * - The capsule first widens leftward anchored at its right edge, then shifts right to recenter
 *
 * These values only describe "when and where"; the actual springs are handed to framer-motion by
 * LiveTranscriptPill.
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
  /** UTF-16 offset in the canonical transcript, when this unit came from a bounded window. */
  sourceOffset?: number;
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
 * Align on the longest common prefix + suffix, giving stable graphemes stable keys.
 * Newly inserted graphemes in the middle are marked born; the next round flattens them to
 * born: false.
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

export const INSERT_TEXT_RENDER_WINDOW = {
  maxUnits: 64,
  overflowBufferUnits: 3,
} as const;

export interface InsertTextWindowOptions {
  maxUnits?: number;
  overflowBufferUnits?: number;
}

function normalizedInsertTextWindowLimit(
  options: InsertTextWindowOptions = {},
): { maxUnits: number; targetUnits: number } {
  const maxUnits = Math.max(1, Math.floor(options.maxUnits ?? INSERT_TEXT_RENDER_WINDOW.maxUnits));
  const overflowBufferUnits = Math.max(
    0,
    Math.floor(options.overflowBufferUnits ?? INSERT_TEXT_RENDER_WINDOW.overflowBufferUnits),
  );
  return { maxUnits, targetUnits: maxUnits + overflowBufferUnits };
}

/**
 * Return only the suffix needed to seed the bounded animation list.
 *
 * The probe starts near the end of the string and expands only when a grapheme is unusually
 * large. This keeps the ordinary streaming path independent of the already-scrolled transcript;
 * the full string remains available to the caller for recognition corrections and accessibility.
 */
export function selectInsertTextWindow(
  text: string,
  options: InsertTextWindowOptions = {},
): string {
  return selectInsertTextWindowSlice(text, options).text;
}

interface InsertTextWindowGlyph {
  text: string;
  sourceOffset: number;
}

interface InsertTextWindowSlice {
  text: string;
  glyphs: InsertTextWindowGlyph[];
}

function selectInsertTextWindowSlice(
  text: string,
  options: InsertTextWindowOptions = {},
): InsertTextWindowSlice {
  if (!text) return { text: '', glyphs: [] };
  const { targetUnits } = normalizedInsertTextWindowLimit(options);
  let probeLength = Math.min(text.length, Math.max(32, targetUnits * 4));

  while (true) {
    const start = text.length - probeLength;
    const segmented = segmentInsertUnits(text.slice(start));
    // A bounded slice can start in the middle of a grapheme. Drop that guard segment; when the
    // slice starts at zero there is no preceding context and the first segment is trustworthy.
    const safeSegments = start === 0 ? segmented : segmented.slice(1);
    if (safeSegments.length >= targetUnits || start === 0) {
      const windowSegments = safeSegments.slice(-targetUnits);
      const safeStartOffset = start + (start === 0 ? 0 : segmented[0]?.length ?? 0);
      const discardedUnits = safeSegments.length - windowSegments.length;
      let windowStartOffset = safeStartOffset;
      for (let i = 0; i < discardedUnits; i += 1) {
        windowStartOffset += safeSegments[i].length;
      }
      const glyphs: InsertTextWindowGlyph[] = [];
      let sourceOffset = windowStartOffset;
      for (const glyph of windowSegments) {
        glyphs.push({ text: glyph, sourceOffset });
        sourceOffset += glyph.length;
      }
      return { text: windowSegments.join(''), glyphs };
    }
    const nextProbeLength = Math.min(text.length, Math.max(probeLength + 32, probeLength * 2));
    if (nextProbeLength === probeLength) {
      const windowSegments = safeSegments.slice(-targetUnits);
      const safeStartOffset = start + (start === 0 ? 0 : segmented[0]?.length ?? 0);
      const discardedUnits = safeSegments.length - windowSegments.length;
      let windowStartOffset = safeStartOffset;
      for (let i = 0; i < discardedUnits; i += 1) {
        windowStartOffset += safeSegments[i].length;
      }
      const glyphs: InsertTextWindowGlyph[] = [];
      let sourceOffset = windowStartOffset;
      for (const glyph of windowSegments) {
        glyphs.push({ text: glyph, sourceOffset });
        sourceOffset += glyph.length;
      }
      return { text: windowSegments.join(''), glyphs };
    }
    probeLength = nextProbeLength;
  }
}

/**
 * Diff only the bounded suffix used by the motion track. The caller may keep the full nextText,
 * but no complete-history InsertUnit array is created or compared here.
 */
export function diffInsertWindowUnits(
  prev: InsertUnit[],
  nextText: string,
  options: InsertTextWindowOptions = {},
): InsertUnit[] {
  const nextWindow = selectInsertTextWindowSlice(nextText, options);
  if (nextWindow.glyphs.length === 0) return [];

  // The suffix text is not a stable identity: once the window rolls, every glyph changes index
  // even though all but the newest glyph are still on screen. Match the bounded overlap by its
  // absolute UTF-16 offset first, so a scrolling deque reuses the old keys instead of replaying
  // the born animation for the whole window.
  const previousByOffset = new Map<number, InsertUnit>();
  for (const unit of prev) {
    if (unit.sourceOffset != null) previousByOffset.set(unit.sourceOffset, unit);
  }
  let exactOverlap = 0;
  for (const glyph of nextWindow.glyphs) {
    const previous = previousByOffset.get(glyph.sourceOffset);
    if (previous?.text === glyph.text) exactOverlap += 1;
  }

  if (exactOverlap > 0 || prev.length === 0) {
    return nextWindow.glyphs.map((glyph) => {
      const previous = previousByOffset.get(glyph.sourceOffset);
      if (previous?.text === glyph.text) {
        return { ...previous, sourceOffset: glyph.sourceOffset, born: false };
      }
      return {
        key: allocInsertKey(),
        text: glyph.text,
        sourceOffset: glyph.sourceOffset,
        born: true,
      };
    });
  }

  // A recognition correction can change the UTF-16 length before the bounded suffix. In that
  // case absolute offsets move together; the bounded local diff still preserves its common
  // prefix/suffix without ever inspecting the discarded transcript history.
  return diffInsertUnits(prev, nextWindow.text).map((unit, index) => ({
    ...unit,
    sourceOffset: nextWindow.glyphs[index]?.sourceOffset,
  }));
}

export interface InsertRenderWindow {
  units: InsertUnit[];
  widths: number[];
}

/**
 * Select only the rightmost glyphs needed by the visual track.
 *
 * The input list is already a bounded animation history. A few extra units form a small left-side
 * buffer for the mask/spring without allowing DOM growth.
 */
export function selectInsertRenderWindow(
  units: InsertUnit[],
  measureWidth: (text: string) => number,
  maxWidth: number,
  padX: number = INSERT_TEXT_MOTION.padX,
  options: {
    maxUnits?: number;
    overflowBufferUnits?: number;
  } = {},
): InsertRenderWindow {
  if (units.length === 0) return { units: [], widths: [] };

  const innerWidth = Math.max(0, maxWidth - padX * 2);
  const maxUnits = Math.max(1, Math.floor(options.maxUnits ?? INSERT_TEXT_RENDER_WINDOW.maxUnits));
  const overflowBufferUnits = Math.max(
    0,
    Math.floor(options.overflowBufferUnits ?? INSERT_TEXT_RENDER_WINDOW.overflowBufferUnits),
  );
  const selectedWidths: number[] = [];
  let start = units.length;
  let contentWidth = 0;
  let bufferUnits = 0;

  for (let index = units.length - 1; index >= 0 && units.length - index <= maxUnits; index -= 1) {
    const measured = measureWidth(units[index].text);
    const width = Number.isFinite(measured) && measured > 0 ? measured : 1;
    const stillVisible = contentWidth < innerWidth;
    if (!stillVisible && bufferUnits >= overflowBufferUnits) break;
    start = index;
    selectedWidths.unshift(width);
    contentWidth += width;
    if (contentWidth >= innerWidth) bufferUnits += 1;
  }

  return {
    units: units.slice(start),
    widths: selectedWidths,
  };
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
