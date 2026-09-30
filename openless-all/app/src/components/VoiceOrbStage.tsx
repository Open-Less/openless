import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { SiriGL } from './SiriGL';
import { getCapsulePillMetrics } from '../lib/capsuleLayout';
import type { CapsuleState } from '../lib/types';
import type { OS } from './WindowChrome';

export interface VoiceOrbStageProps {
  os: OS;
  state: CapsuleState;
  level: number;
  /** Warming: the wave renders as a standby breathing form, ignoring the real level. See CapsulePayload.warming. */
  warming?: boolean;
  /** Average warm→ready duration (ms), driving the predicted expand pace. See SiriGL warmupMs. */
  warmupMs?: number;
  message?: string;
}

/**
 * Pure light stage (full siri-glsl clone, no chrome, no buttons, no backdrop):
 *   - recording: a rainbow spectral voice wave spans the stage, amplitude following
 *     the real mic level;
 *   - transcribing / polishing: the wave collapses from both ends toward the center
 *     while a fluid dot ring fades in and spins faster;
 *   - done / cancelled: speed falls back to standard, the six dots merge into one
 *     center circle, and the outer capsule-out fades it away;
 *   - error: frozen light + a glowing red line explaining why (the only text kept).
 * Deliberately no underlay/dark vignette: on a white UI, weaker contrast beats a
 * black occlusion.
 */
export function VoiceOrbStage({
  os,
  state,
  level,
  warming,
  warmupMs,
  message,
}: VoiceOrbStageProps) {
  const { t } = useTranslation();
  const metrics = useMemo(() => getCapsulePillMetrics(os), [os]);

  // done / cancelled / error freeze the last phase and fade out; no further switching.
  const lastPhaseRef = useRef<'wave' | 'orb'>('wave');
  let phase = lastPhaseRef.current;
  if (state === 'recording') phase = 'wave';
  else if (state === 'transcribing' || state === 'polishing') phase = 'orb';
  lastPhaseRef.current = phase;
  const isOrb = phase === 'orb';

  // Perf: unmount the wave's draw loop after its fade fully ends (.55s delay + .6s
  // duration) so thinking doesn't run fragments every frame for an invisible canvas.
  // Remounts immediately on returning to recording (the driver caches the compiled
  // shader; rebuild is near-zero cost).
  const [waveAlive, setWaveAlive] = useState(true);
  useEffect(() => {
    if (!isOrb) {
      setWaveAlive(true);
      return undefined;
    }
    const timer = setTimeout(() => setWaveAlive(false), 1300);
    return () => clearTimeout(timer);
  }, [isOrb]);

  return (
    <div
      style={{
        width: metrics.width,
        height: metrics.height,
        boxSizing: metrics.boxSizing,
        fontFamily: 'var(--ol-font-sans)',
        position: 'relative',
        pointerEvents: 'none',
      }}
    >
      {waveAlive && (
        <SiriGL
          mode="wave"
          level={level}
          resolved={!isOrb}
          warming={warming}
          warmupMs={warmupMs}
          style={{
            position: 'absolute',
            inset: 0,
            width: '100%',
            height: '100%',
            // Keep the wave visible while it collapses; fade out once it becomes the
            // center orb, overlapping the dot ring's fade-in.
            opacity: isOrb ? 0 : 1,
            transition: isOrb ? 'opacity .6s ease-out .55s' : 'opacity .25s ease-out',
          }}
        />
      )}
      {isOrb && (
        <SiriGL
          mode="orb"
          // Spin faster while thinking (LLM is receiving); on insert/cancel/error fall
          // back to standard speed as the six dots merge into one center circle and
          // vanish with the outer fade.
          speed={state === 'transcribing' || state === 'polishing' ? 1.5 : 1.0}
          merging={state !== 'transcribing' && state !== 'polishing'}
          style={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            width: 170,
            height: 170,
            marginLeft: -85,
            marginTop: -85,
            animation: 'siri-orb-in .7s ease-out .3s both',
          }}
        />
      )}
      {state === 'error' && <span style={errorGlowTextStyle}>{message || t('capsule.error')}</span>}
    </div>
  );
}

const errorGlowTextStyle: CSSProperties = {
  position: 'absolute',
  bottom: 24,
  left: '50%',
  transform: 'translateX(-50%)',
  maxWidth: 400,
  fontSize: 12,
  fontWeight: 600,
  lineHeight: 1.4,
  textAlign: 'center',
  color: 'var(--ol-err)',
  padding: '6px 12px',
  background: 'var(--ol-capsule-pill-bg)',
  border: '1px solid var(--ol-capsule-pill-border)',
  borderRadius: 12,
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
};
