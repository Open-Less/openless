import { useCallback, useRef, useState, type KeyboardEvent, type PointerEvent, type WheelEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  DEFAULT_CAPSULE_SENSITIVITY,
  MIN_CAPSULE_SENSITIVITY,
  MAX_CAPSULE_SENSITIVITY,
  STEP_CAPSULE_SENSITIVITY,
  clampCapsuleSensitivity,
} from '../../lib/capsuleSensitivity';
import './VerticalSensitivityRegulator.css';

export interface VerticalSensitivityRegulatorProps {
  value: number;
  onChange: (value: number) => void;
  className?: string;
}

/**
 * Compact vertical regulator slider placed next to the capsule style preview.
 *
 * Interactions:
 * - Drag vertically to adjust sensitivity (0.5× ~ 2.5×).
 * - Mouse wheel to step up/down (+0.1×).
 * - Double-click to instantly reset to the default 1.0×.
 * - Keyboard accessible: ArrowUp/Down/Left/Right, Home/End, PageUp/Down.
 */
export function VerticalSensitivityRegulator({
  value,
  onChange,
  className,
}: VerticalSensitivityRegulatorProps) {
  const { t } = useTranslation();
  const trackRef = useRef<HTMLDivElement>(null);
  const [isDragging, setIsDragging] = useState(false);

  const currentValue = clampCapsuleSensitivity(value);

  // Fraction in [0, 1] from min to max
  const fraction =
    (currentValue - MIN_CAPSULE_SENSITIVITY) /
    (MAX_CAPSULE_SENSITIVITY - MIN_CAPSULE_SENSITIVITY);

  const updateFromPointer = useCallback(
    (clientY: number) => {
      const track = trackRef.current;
      if (!track) return;
      const rect = track.getBoundingClientRect();
      // Y grows downwards; slider bottom is min, top is max
      const ratio = 1 - (clientY - rect.top) / rect.height;
      const clampedRatio = Math.min(1, Math.max(0, ratio));
      const rawVal =
        MIN_CAPSULE_SENSITIVITY +
        clampedRatio * (MAX_CAPSULE_SENSITIVITY - MIN_CAPSULE_SENSITIVITY);
      const stepped = clampCapsuleSensitivity(rawVal);
      if (stepped !== currentValue) {
        onChange(stepped);
      }
    },
    [currentValue, onChange],
  );

  const handlePointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDragging(true);
    e.currentTarget.setPointerCapture(e.pointerId);
    updateFromPointer(e.clientY);
  };

  const handlePointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!isDragging) return;
    updateFromPointer(e.clientY);
  };

  const handlePointerUp = (e: PointerEvent<HTMLDivElement>) => {
    if (isDragging) {
      setIsDragging(false);
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        // Safe to ignore if pointer already released
      }
    }
  };

  const handleDoubleClick = () => {
    onChange(DEFAULT_CAPSULE_SENSITIVITY);
  };

  const handleWheel = (e: WheelEvent<HTMLDivElement>) => {
    e.preventDefault();
    const delta = e.deltaY < 0 ? STEP_CAPSULE_SENSITIVITY : -STEP_CAPSULE_SENSITIVITY;
    const next = clampCapsuleSensitivity(currentValue + delta);
    if (next !== currentValue) {
      onChange(next);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    let next: number | null = null;
    if (e.key === 'ArrowUp' || e.key === 'ArrowRight') {
      next = clampCapsuleSensitivity(currentValue + STEP_CAPSULE_SENSITIVITY);
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') {
      next = clampCapsuleSensitivity(currentValue - STEP_CAPSULE_SENSITIVITY);
    } else if (e.key === 'Home') {
      next = MIN_CAPSULE_SENSITIVITY;
    } else if (e.key === 'End') {
      next = MAX_CAPSULE_SENSITIVITY;
    } else if (e.key === 'PageUp') {
      next = clampCapsuleSensitivity(currentValue + STEP_CAPSULE_SENSITIVITY * 3);
    } else if (e.key === 'PageDown') {
      next = clampCapsuleSensitivity(currentValue - STEP_CAPSULE_SENSITIVITY * 3);
    }
    if (next !== null && next !== currentValue) {
      e.preventDefault();
      onChange(next);
    }
  };

  const tooltipText = t('settings.recording.capsuleSensitivityTooltip');

  return (
    <div
      className={`ol-sensitivity-regulator ${className ?? ''}`}
      role="slider"
      tabIndex={0}
      aria-label={t('settings.recording.capsuleSensitivityLabel')}
      aria-valuenow={currentValue}
      aria-valuemin={MIN_CAPSULE_SENSITIVITY}
      aria-valuemax={MAX_CAPSULE_SENSITIVITY}
      aria-valuetext={`${currentValue.toFixed(1)}×`}
      title={tooltipText}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      onDoubleClick={handleDoubleClick}
      onWheel={handleWheel}
      onKeyDown={handleKeyDown}
      data-dragging={isDragging}
    >
      <div className="ol-sensitivity-header" aria-hidden="true">
        <svg
          className="ol-sensitivity-icon"
          viewBox="0 0 16 16"
          width="12"
          height="12"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
        >
          <path d="M2 8h1M5 5v6M8 3v10M11 6v4M14 8h1" />
        </svg>
      </div>
      <div className="ol-sensitivity-track-wrap" ref={trackRef}>
        <div className="ol-sensitivity-track">
          <div
            className="ol-sensitivity-fill"
            style={{ height: `${Math.round(fraction * 100)}%` }}
          />
          <div
            className="ol-sensitivity-thumb"
            style={{ bottom: `calc(${Math.round(fraction * 100)}% - 5px)` }}
          />
        </div>
      </div>
      <div className="ol-sensitivity-value" aria-hidden="true">
        {currentValue.toFixed(1)}×
      </div>
    </div>
  );
}
