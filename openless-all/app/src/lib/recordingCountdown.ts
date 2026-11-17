/** Seconds left before the recording auto-stops; below this the badge turns urgent. */
export const RECORDING_COUNTDOWN_URGENT_SECONDS = 10;

/**
 * Render the backend-provided countdown for the capsule badge.
 *
 * The backend only publishes `countdownSeconds` during the final minute of a
 * recording (see `openless_core::recording_limit`), so this is presentation
 * only: it formats the value and rejects the sentinel values a stale or
 * partially-decoded payload can carry.
 */
export function formatRecordingCountdown(seconds: number | null | undefined): string | null {
  if (typeof seconds !== 'number' || !Number.isFinite(seconds)) return null;
  const whole = Math.floor(seconds);
  if (whole < 0) return null;
  const minutes = Math.floor(whole / 60);
  const rest = whole % 60;
  return `${minutes}:${String(rest).padStart(2, '0')}`;
}

/** True once the session is close enough to the cap that the badge should shout. */
export function recordingCountdownUrgent(seconds: number | null | undefined): boolean {
  return (
    typeof seconds === 'number' &&
    Number.isFinite(seconds) &&
    seconds >= 0 &&
    seconds <= RECORDING_COUNTDOWN_URGENT_SECONDS
  );
}
