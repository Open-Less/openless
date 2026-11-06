import { useTranslation } from 'react-i18next';
import { formatRecordingCountdown, recordingCountdownUrgent } from '../lib/recordingCountdown';
import './CapsuleStyles.css';

interface RecordingCountdownBadgeProps {
  /** Seconds left before the hard recording cap, or null when the cap is not close. */
  seconds?: number | null;
}

/**
 * "Time left" chip for the final minute of a recording. The backend decides when the
 * countdown exists (see `openless_core::recording_limit`); this only presents it, so an
 * absent value renders nothing rather than a stale zero.
 */
export function RecordingCountdownBadge({ seconds }: RecordingCountdownBadgeProps) {
  const { t } = useTranslation();
  const text = formatRecordingCountdown(seconds);
  if (text === null) return null;
  const label = t('capsule.recordingCountdown', { time: text });
  return (
    <div
      className="ol-recording-countdown"
      data-urgent={recordingCountdownUrgent(seconds)}
      role="timer"
      aria-label={label}
      title={label}
    >
      <span className="ol-recording-countdown-dot" aria-hidden="true" />
      <span className="ol-recording-countdown-value">{text}</span>
    </div>
  );
}
