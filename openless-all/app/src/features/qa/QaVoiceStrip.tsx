import { MicIcon } from 'lucide-react';
import { useEffect, useState } from 'react';

interface Props {
  level: number;
  recording: boolean;
  label?: string;
}

const BAR_SHAPE = [
  0.25, 0.5, 0.65, 0.9, 0.55, 1, 0.72, 0.43, 0.81, 0.62, 0.94, 0.49, 0.31, 0.57, 0.75, 0.32,
];

export function QaVoiceStrip({ level, recording, label }: Props) {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    if (!recording) {
      setSeconds(0);
      return;
    }
    const start = performance.now();
    const timer = setInterval(
      () => setSeconds(Math.floor((performance.now() - start) / 1000)),
      250,
    );
    return () => clearInterval(timer);
  }, [recording]);
  const elapsed = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
  return (
    <div className="qa-voice-strip" data-recording={recording || undefined}>
      <span className="qa-voice-strip-icon" aria-hidden="true">
        <MicIcon />
      </span>
      <span className="qa-voice-strip-bars" aria-hidden="true">
        {BAR_SHAPE.map((weight, i) => (
          <i
            key={i}
            style={{
              height: `${recording ? 3 + 17 * weight * Math.max(0, Math.min(1, level)) : 3}px`,
            }}
          />
        ))}
      </span>
      {recording && <time className="qa-voice-strip-time">{elapsed}</time>}
      {label && (
        <span className="voice-waveform-label qa-voice-strip-label" role="status">
          {label}
        </span>
      )}
    </div>
  );
}
