import { useRef, type ReactNode } from 'react';
import { useSelectionMotion } from '../../lib/motion';

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  title?: string;
  icon?: ReactNode;
  description?: string;
}

/** The selected chip moves under the stable button labels. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  disabled = false,
  compact = false,
  optionClassName,
  className,
  indicatorClassName,
}: {
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
  disabled?: boolean;
  compact?: boolean;
  optionClassName?: string;
  className?: string;
  /** Extra hook for callers whose tests address the sliding chip directly. */
  indicatorClassName?: string;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const indicatorRef = useRef<HTMLSpanElement>(null);
  useSelectionMotion(trackRef, indicatorRef, value);
  return (
    <div
      ref={trackRef}
      role="group"
      aria-label={label}
      className={['ol-segmented', className].filter(Boolean).join(' ')}
      data-compact={compact || undefined}
    >
      <span
        ref={indicatorRef}
        aria-hidden
        className={['ol-segmented-indicator', indicatorClassName].filter(Boolean).join(' ')}
      />
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={['ol-segmented-option', optionClassName].filter(Boolean).join(' ')}
          aria-pressed={option.value === value}
          disabled={disabled}
          title={option.title}
          onClick={() => {
            if (option.value !== value) onChange(option.value);
          }}
        >
          {option.icon && <span className="ol-segmented-icon" aria-hidden>{option.icon}</span>}
          <span className="ol-segmented-label">{option.label}</span>
          {option.description && (
            <span className="ol-segmented-description">{option.description}</span>
          )}
        </button>
      ))}
    </div>
  );
}
