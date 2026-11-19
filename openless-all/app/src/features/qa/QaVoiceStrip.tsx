import { MicIcon } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

interface Props {
  level: number;
  elapsedMs?: number;
  contentScale: number;
  label: string;
  stopLabel: string;
  disabled: boolean;
  onStop: () => void;
}

const BAR_COUNT = 40;
const SAMPLE_MS = 75;

export function QaVoiceStrip({
  level,
  elapsedMs,
  contentScale,
  label,
  stopLabel,
  disabled,
  onStop,
}: Props) {
  const [samples, setSamples] = useState<number[]>(() => Array(BAR_COUNT).fill(0));
  const [seconds, setSeconds] = useState(0);
  const microphone = useRef(0);
  microphone.current = Number.isFinite(level) ? Math.min(1, Math.max(0, level)) : 0;
  useEffect(() => {
    const start = performance.now();
    const timer = setInterval(() => {
      // One new microphone sample enters on the right. Older samples keep their own volume.
      setSamples((history) => [...history.slice(1), microphone.current]);
      setSeconds(Math.floor((performance.now() - start) / 1000));
    }, SAMPLE_MS);
    return () => clearInterval(timer);
  }, []);
  const totalSeconds = elapsedMs == null ? seconds : Math.max(0, Math.floor(elapsedMs / 1000));
  const elapsed = `${Math.floor(totalSeconds / 60)}:${String(totalSeconds % 60).padStart(2, '0')}`;
  return (
    <div
      className="qa-voice-strip"
      style={contentScale > 1 ? { zoom: 1 / contentScale } : undefined}
    >
      <button
        type="button"
        className="qa-voice-strip-control"
        disabled={disabled}
        aria-label={stopLabel}
        title={stopLabel}
        onMouseDown={(event) => {
          if (event.button === 0) event.preventDefault();
        }}
        onClick={onStop}
      >
        <MicIcon />
      </button>
      <span className="qa-voice-strip-bars" aria-hidden="true">
        {samples.map((sample, index) => (
          <i key={index} style={{ height: `${5 + 26 * Math.sqrt(sample)}px` }} />
        ))}
      </span>
      <time className="qa-voice-strip-time">{elapsed}</time>
      <span className="voice-waveform-label sr-only" role="status">
        {label}
      </span>
    </div>
  );
}
