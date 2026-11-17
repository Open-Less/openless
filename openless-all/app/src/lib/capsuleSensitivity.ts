export const DEFAULT_CAPSULE_SENSITIVITY = 1.0;
export const MIN_CAPSULE_SENSITIVITY = 0.5;
export const MAX_CAPSULE_SENSITIVITY = 2.5;
export const STEP_CAPSULE_SENSITIVITY = 0.1;

export const FAN_NOISE_GATE = 0.012;

/**
 * Clamps and rounds capsule sensitivity to the valid range [0.5, 2.5] with 0.1 step.
 * Falls back to 1.0 for null, undefined or non-finite inputs.
 */
export function clampCapsuleSensitivity(val: unknown): number {
  if (typeof val !== 'number' || !Number.isFinite(val)) {
    return DEFAULT_CAPSULE_SENSITIVITY;
  }
  const clamped = Math.min(
    MAX_CAPSULE_SENSITIVITY,
    Math.max(MIN_CAPSULE_SENSITIVITY, val),
  );
  return Math.round(clamped * 10) / 10;
}

/**
 * Psychoacoustic voice level mapping with active fan noise suppression and
 * expanded dynamic range for quiet speech.
 *
 * - Idle / fan noise (< 0.012 RMS): strictly suppressed to 0, keeping the wave calm.
 * - Soft speech (0.03 ~ 0.08 RMS): scaled dynamically according to user sensitivity.
 * - Loud speech: smoothly caps at 1.0 without sudden jumps.
 */
export function mapVoiceLevel(raw: number, sensitivity: unknown = DEFAULT_CAPSULE_SENSITIVITY): number {
  if (!Number.isFinite(raw) || raw <= FAN_NOISE_GATE) {
    return 0;
  }

  const sens = clampCapsuleSensitivity(sensitivity);

  // Dynamic ceiling lowers with higher sensitivity so lower voice reaches peak dynamics
  // At sens = 0.5, ceiling = 0.36; at 1.0, ceiling = 0.24; at 2.5, ceiling = 0.12.
  const ceiling = Math.max(0.10, 0.24 / Math.sqrt(sens));
  const span = ceiling - FAN_NOISE_GATE;
  const normalized = Math.min(1, Math.max(0, (raw - FAN_NOISE_GATE) / span));

  // Smoothstep ease-in-out curve
  const eased = normalized * normalized * (3 - 2 * normalized);

  // Power curve expands the perceptual loudness of soft voice
  // Higher sensitivity softens the power curve exponent for even faster onset
  const exponent = 0.38 / Math.pow(sens, 0.25);
  const result = Math.pow(eased, exponent);

  return Math.min(1, Math.max(0, result));
}
