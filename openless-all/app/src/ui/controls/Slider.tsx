import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { Slider as BaseSlider } from '@base-ui/react/slider';
import { DirectionProvider } from '@base-ui/react/direction-provider';
import './slider.css';

export type SliderValue = number | readonly number[];
export type SliderVariant = 'simple' | 'square' | 'solid' | 'tiny';
export interface SliderMark {
  value: number;
  label?: ReactNode;
}
export interface SliderProps<Value extends SliderValue = number> extends Omit<
  BaseSlider.Root.Props<Value>,
  'children' | 'className' | 'style' | 'onValueChange' | 'onValueCommitted'
> {
  /** Every thumb receives this accessible name, or a name from thumbLabels. */
  'aria-label': string;
  thumbLabels?: readonly string[];
  onValueChange?: (value: Value) => void;
  onValueCommitted?: (value: Value) => void;
  variant?: SliderVariant;
  direction?: 'ltr' | 'rtl';
  className?: string;
  style?: CSSProperties;
  marks?: readonly SliderMark[];
  ticks?: boolean;
  tooltip?: boolean;
  endpointLabels?: readonly [ReactNode, ReactNode];
}

function useSliderValue<Value extends SliderValue>(
  value: Value | undefined,
  defaultValue: Value,
  onChange: ((next: Value) => void) | undefined,
) {
  const [internal, setInternal] = useState(defaultValue);
  const current = value === undefined ? internal : value;
  const change = (next: Value) => {
    if (value === undefined) setInternal(next);
    onChange?.(next);
  };
  return [current, change] as const;
}

/** Base UI owns pointer capture, native inputs, keyboard handling and commit timing. */
export function Slider<Value extends SliderValue = number>({
  value,
  defaultValue,
  onValueChange,
  onValueCommitted,
  variant = 'simple',
  direction,
  className = '',
  style,
  marks,
  ticks = false,
  tooltip = false,
  endpointLabels,
  thumbLabels,
  'aria-label': label,
  min = 0,
  max = 100,
  ...props
}: SliderProps<Value>) {
  const [current, change] = useSliderValue(value, defaultValue ?? (min as Value), onValueChange);
  const values: readonly number[] = typeof current === 'number' ? [current] : current;
  const content = (
    <BaseSlider.Root
      {...props}
      min={min}
      max={max}
      value={current}
      onValueChange={(next) => change(next as Value)}
      onValueCommitted={(next) => onValueCommitted?.(next as Value)}
      className={`ol-slider ol-slider--${variant} ${className}`}
      style={style}
      dir={direction}
    >
      {endpointLabels && (
        <div className="ol-slider-endpoints" aria-hidden="true">
          <span>{endpointLabels[0]}</span>
          <span>{endpointLabels[1]}</span>
        </div>
      )}
      <BaseSlider.Control className="ol-slider-control">
        <BaseSlider.Track className="ol-slider-track">
          <BaseSlider.Indicator className="ol-slider-indicator" />
          {values.map((item, index) => (
            <BaseSlider.Thumb
              key={index}
              index={index}
              className="ol-slider-thumb"
              aria-label={
                thumbLabels?.[index] ?? (values.length > 1 ? `${label} ${index + 1}` : label)
              }
              aria-describedby={props['aria-describedby']}
            >
              {tooltip && (
                <span className="ol-slider-tooltip" aria-hidden="true">
                  {item}
                </span>
              )}
            </BaseSlider.Thumb>
          ))}
        </BaseSlider.Track>
      </BaseSlider.Control>
      {marks && (
        <div className="ol-slider-marks" aria-hidden="true">
          {marks
            .filter((mark) => mark.value >= min && mark.value <= max)
            .map((mark) => (
              <span
                key={mark.value}
                className={ticks ? 'ol-slider-mark ol-slider-mark--tick' : 'ol-slider-mark'}
                style={{ insetInlineStart: `${((mark.value - min) / (max - min)) * 100}%` }}
              >
                {mark.label}
              </span>
            ))}
        </div>
      )}
    </BaseSlider.Root>
  );
  return direction ? (
    <DirectionProvider direction={direction}>{content}</DirectionProvider>
  ) : (
    content
  );
}

export function SimpleSlider<Value extends SliderValue = number>(props: SliderProps<Value>) {
  return <Slider {...props} variant="simple" />;
}
export function SquareSlider<Value extends SliderValue = number>(props: SliderProps<Value>) {
  return <Slider {...props} variant="square" />;
}
export function SolidSlider<Value extends SliderValue = number>(props: SliderProps<Value>) {
  return <Slider {...props} variant="solid" />;
}
export function TinySlider<Value extends SliderValue = number>(props: SliderProps<Value>) {
  return <Slider {...props} variant="tiny" />;
}
export function ReferenceSlider<Value extends SliderValue = number>({
  min = 0,
  max = 100,
  ...props
}: SliderProps<Value>) {
  return (
    <Slider
      min={min}
      max={max}
      marks={[
        { value: min, label: min },
        { value: (min + max) / 2, label: (min + max) / 2 },
        { value: max, label: max },
      ]}
      {...props}
    />
  );
}
export function TickSlider<Value extends SliderValue = number>({
  min = 0,
  max = 12,
  ...props
}: SliderProps<Value>) {
  return (
    <Slider
      min={min}
      max={max}
      marks={Array.from({ length: 13 }, (_, index) => ({
        value: min + ((max - min) * index) / 12,
        label: index % 2 === 0 ? min + ((max - min) * index) / 12 : undefined,
      }))}
      ticks
      {...props}
    />
  );
}
export function TooltipSlider<Value extends SliderValue = number>(props: SliderProps<Value>) {
  return <Slider endpointLabels={['Low', 'High']} {...props} tooltip />;
}
export function RangeSlider(props: SliderProps<readonly number[]>) {
  return <Slider defaultValue={[25, 75]} thumbCollisionBehavior="none" {...props} />;
}
export function VerticalSlider<Value extends SliderValue = number>(props: SliderProps<Value>) {
  return <Slider {...props} orientation="vertical" />;
}

export function StepperSlider({
  value,
  defaultValue = 50,
  onValueChange,
  onValueCommitted,
  min = 0,
  max = 100,
  step = 1,
  decreaseLabel = 'Decrease value',
  increaseLabel = 'Increase value',
  ...props
}: SliderProps<number> & { decreaseLabel?: string; increaseLabel?: string }) {
  const [current, change] = useSliderValue(value, defaultValue, onValueChange);
  const move = (delta: number) => {
    const next = Math.min(
      max,
      Math.max(
        min,
        Number((min + Math.round((current + delta - min) / step) * step).toPrecision(12)),
      ),
    );
    if (next === current) return;
    change(next);
    onValueCommitted?.(next);
  };
  return (
    <div className="ol-slider-stepper">
      <button
        type="button"
        aria-label={decreaseLabel}
        disabled={props.disabled || current <= min}
        onClick={() => move(-step)}
      >
        −
      </button>
      <Slider
        {...props}
        min={min}
        max={max}
        step={step}
        value={current}
        onValueChange={change}
        onValueCommitted={onValueCommitted}
      />
      <button
        type="button"
        aria-label={increaseLabel}
        disabled={props.disabled || current >= max}
        onClick={() => move(step)}
      >
        +
      </button>
    </div>
  );
}

export interface DistributionSliderProps extends SliderProps<readonly number[]> {
  /** Equal-width, caller-supplied counts over [min, max]. No sample data is fabricated. */
  bins?: readonly number[];
  inputLabels?: readonly [string, string];
}

function RangeValueInput({
  value,
  min,
  max,
  step,
  disabled,
  label,
  onCommit,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  disabled?: boolean;
  label: string;
  onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const cancelBlur = useRef(false);
  useEffect(() => setDraft(null), [value, min, max, step]);
  const commit = (text: string) => {
    setDraft(null);
    if (!text.trim() || !Number.isFinite(Number(text))) return;
    const next = Math.min(
      max,
      Math.max(min, Number((min + Math.round((Number(text) - min) / step) * step).toPrecision(12))),
    );
    if (next !== value) onCommit(next);
  };
  return (
    <label className="ol-slider-value-input">
      <span>{label}</span>
      <input
        type="text"
        inputMode="decimal"
        aria-label={label}
        disabled={disabled}
        value={draft ?? String(value)}
        onChange={(event) => setDraft(event.target.value)}
        onFocus={() => setDraft(String(value))}
        onBlur={(event) => {
          if (cancelBlur.current) {
            cancelBlur.current = false;
            return;
          }
          if (draft !== null) commit(event.currentTarget.value);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            commit(event.currentTarget.value);
          } else if (event.key === 'Escape') {
            cancelBlur.current = true;
            setDraft(null);
            event.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}

export function DistributionSlider({
  value,
  defaultValue = [25, 75],
  onValueChange,
  onValueCommitted,
  bins = [],
  inputLabels = ['Minimum value', 'Maximum value'],
  min = 0,
  max = 100,
  step = 1,
  ...props
}: DistributionSliderProps) {
  const [current, change] = useSliderValue(value, defaultValue, onValueChange);
  const low = current[0] ?? min;
  const high = current[1] ?? max;
  const maximumCount = Math.max(1, ...bins.filter(Number.isFinite));
  const commitInput = (index: number, next: number) => {
    const result = index === 0 ? [Math.min(next, high), high] : [low, Math.max(next, low)];
    if (result[0] === low && result[1] === high) return;
    change(result);
    onValueCommitted?.(result);
  };
  return (
    <div className="ol-slider-distribution" dir={props.direction}>
      {bins.length > 0 && (
        <div className="ol-slider-histogram" aria-hidden="true">
          {bins.map((count, index) => (
            <span
              key={index}
              data-selected={
                min + (index / bins.length) * (max - min) <= high &&
                min + ((index + 1) / bins.length) * (max - min) >= low
                  ? ''
                  : undefined
              }
              style={{
                height: `${(Math.max(0, Number.isFinite(count) ? count : 0) / maximumCount) * 100}%`,
              }}
            />
          ))}
        </div>
      )}
      <RangeSlider
        {...props}
        min={min}
        max={max}
        step={step}
        value={current}
        onValueChange={change}
        onValueCommitted={onValueCommitted}
      />
      <div className="ol-slider-inputs">
        {[low, high].map((item, index) => (
          <RangeValueInput
            key={index}
            value={item}
            min={min}
            max={max}
            step={step}
            disabled={props.disabled}
            label={inputLabels[index]}
            onCommit={(next) => commitInput(index, next)}
          />
        ))}
      </div>
    </div>
  );
}
