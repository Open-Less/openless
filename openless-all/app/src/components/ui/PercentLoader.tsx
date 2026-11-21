import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useReducedMotion } from '../../lib/motion';
import './percent-loader.css';

/** Startup and page masks have no measurable total, so the count loops.
 *  Pass `progress` only for a real ratio; that value does not reset. */
export function PercentLoader({
  progress,
  speed = 1,
  size = 48,
}: {
  progress?: number;
  speed?: number;
  size?: number;
}) {
  const { t } = useTranslation();
  const reduce = useReducedMotion();
  const determinate = progress !== undefined;
  const [counted, setCounted] = useState(0);
  useEffect(() => {
    if (determinate) return;
    const duration = (reduce ? speed * 2 : speed) * 1000;
    const clock = { elapsed: 0 };
    const tickMs = 40;
    const id = window.setInterval(() => {
      clock.elapsed += tickMs;
      const next = Math.min(100, Math.round((clock.elapsed / duration) * 100));
      setCounted(next);
      if (next >= 100) clock.elapsed = 0;
    }, tickMs);
    return () => window.clearInterval(id);
  }, [determinate, reduce, speed]);
  const value = determinate ? Math.min(100, Math.max(0, progress)) : counted;
  return (
    <span
      className="ol-percent-loader"
      role="progressbar"
      aria-label={t('common.loading')}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value)}
      style={{ gap: size * 0.14, width: size * 1.4 }}
    >
      <span className="ol-percent-loader-label" style={{ fontSize: size * 0.42 }}>
        {Math.round(value)}%
      </span>
      <span
        className="ol-percent-loader-track"
        aria-hidden="true"
        style={{ height: Math.max(3, size * 0.1) }}
      >
        <span className="ol-percent-loader-fill" style={{ width: `${value}%` }} />
      </span>
    </span>
  );
}
