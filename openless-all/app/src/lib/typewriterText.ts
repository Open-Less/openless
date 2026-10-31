// Typewriter pacing for the live transcript rail.
//
// Streaming ASR delivers text in bursts (every 200-400 ms). The rail still lays out each burst
// at once, so the pill width animates once per burst; only the birth of the new glyphs is
// staggered, so a burst inks in left to right instead of appearing as one block.

export const TYPEWRITER = {
  /**
   * Fastest gap between two glyph births. Kept well under the glyph focus animation
   * (~380 ms) so consecutive glyphs overlap into a wave instead of flashing one by one.
   */
  minStepMs: 45,
  /** Slowest gap, used when only a few glyphs are born in a burst. */
  maxStepMs: 80,
  /** Aim to finish a burst within roughly one ASR update interval. */
  catchUpMs: 400,
} as const;

/** Birth delay per unit: born units are staggered in order, retained units start at once. */
export function planBirthDelays(born: readonly boolean[]): number[] {
  const count = born.filter(Boolean).length;
  if (count === 0) return born.map(() => 0);
  const step = Math.min(
    TYPEWRITER.maxStepMs,
    Math.max(TYPEWRITER.minStepMs, Math.round(TYPEWRITER.catchUpMs / count)),
  );
  let order = 0;
  return born.map((isBorn) => {
    if (!isBorn) return 0;
    const delay = order * step;
    order += 1;
    return delay;
  });
}
