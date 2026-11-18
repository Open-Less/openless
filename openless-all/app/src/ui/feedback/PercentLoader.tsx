import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useReducedMotion } from '../../lib/motion';
import './percent-loader.css';

export function PercentLoader({
  progress,
  size = 48,
  label,
}: {
  progress?: number;
  size?: number;
  label?: string;
}) {
  const { t } = useTranslation();
  const reducedMotion = useReducedMotion();
  const determinate = progress !== undefined && Number.isFinite(progress);
  const [counted, setCounted] = useState(0);

  useEffect(() => {
    if (determinate || reducedMotion) return;
    const start = performance.now();
    const cycle = 1040;
    const timer = window.setInterval(() => {
      setCounted(Math.min(100, Math.round(((performance.now() - start) % cycle) / 10)));
    }, 40);
    return () => window.clearInterval(timer);
  }, [determinate, reducedMotion]);

  const value = Math.round(
    determinate ? Math.min(100, Math.max(0, progress)) : reducedMotion ? 40 : counted,
  );
  return (
    <span
      className="ol-percent-loader"
      role="progressbar"
      aria-label={label ?? t('common.loading')}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={determinate ? value : undefined}
      style={{ gap: size * 0.14, width: size * 1.4 }}
    >
      {(!reducedMotion || determinate) && (
        <span className="ol-percent-loader-label" style={{ fontSize: size * 0.42 }} aria-hidden>
          {value}%
        </span>
      )}
      <span
        className="ol-percent-loader-track"
        aria-hidden
        style={{ height: Math.max(3, size * 0.1) }}
      >
        <span className="ol-percent-loader-fill" style={{ width: `${value}%` }} />
      </span>
    </span>
  );
}
