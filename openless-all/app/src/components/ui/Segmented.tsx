import { useRef } from 'react';
import { useSelectionMotion } from '../../lib/motion';

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  title?: string;
  /** 单个选项在平台/状态下不可用（整条控件仍可操作其它选项）。 */
  disabled?: boolean;
}

/** The one segmented control used across Settings: a white track with the controls'
 * hairline, and a neutral chip that slides under the selected option. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  disabled = false,
  className,
  indicatorClassName,
}: {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
  disabled?: boolean;
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
          className="ol-segmented-option"
          aria-pressed={option.value === value}
          disabled={disabled || option.disabled}
          title={option.title}
          onClick={() => {
            if (option.disabled) return;
            if (option.value !== value) onChange(option.value);
          }}
        >
          {/* data-label reserves the semibold width so selecting never nudges a neighbour. */}
          <span className="ol-segmented-label" data-label={option.label}>
            {option.label}
          </span>
        </button>
      ))}
    </div>
  );
}
