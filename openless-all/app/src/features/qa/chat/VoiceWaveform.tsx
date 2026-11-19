import type { CSSProperties } from 'react';

const WEIGHTS = [
  0.2, 0.4, 0.7, 0.45, 0.9, 0.65, 1, 0.75, 0.5, 0.95, 0.65, 0.4, 0.8, 0.55, 0.35, 0.6, 0.25,
];

export function VoiceWaveform({
  level,
  processing = false,
  label,
}: {
  level: number;
  processing?: boolean;
  label: string;
}) {
  const safeLevel = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  return (
    <div
      className={`voice-waveform${processing ? ' is-processing' : ''}`}
      role="status"
      aria-label={label}
    >
      <div className="voice-waveform-bars" aria-hidden="true">
        {WEIGHTS.map((weight, index) => (
          <span
            key={index}
            style={
              {
                height: `${3 + Math.sqrt(safeLevel) * weight * 25}px`,
                '--wave-delay': `${index * -60}ms`,
              } as CSSProperties
            }
          />
        ))}
      </div>
      <span className="voice-waveform-label">{label}</span>
    </div>
  );
}
