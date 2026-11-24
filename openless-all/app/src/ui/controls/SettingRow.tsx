import { createContext, useContext, useId, type CSSProperties, type ReactNode } from 'react';
import { Switch } from '@base-ui/react/switch';
import { Hint } from '../overlays/Tooltip';
import './setting-row.css';

const SettingLabel = createContext<string | undefined>(undefined);

export interface SettingRowProps {
  label: string;
  desc?: ReactNode;
  children: ReactNode;
  controlWidth?: number | string;
  className?: string;
  descriptionPlacement?: 'hint' | 'inline';
}

export function SettingRow({
  label,
  desc,
  children,
  controlWidth,
  className,
  descriptionPlacement = 'hint',
}: SettingRowProps) {
  const labelId = useId();
  return (
    <div
      className={['ol-setting-row', className].filter(Boolean).join(' ')}
      data-description-placement={descriptionPlacement}
      style={
        {
          '--ol-row-control-width':
            typeof controlWidth === 'number' ? `${controlWidth}px` : controlWidth,
        } as CSSProperties
      }
    >
      <div className="ol-setting-row-label">
        {desc && descriptionPlacement === 'hint' ? (
          <Hint id={labelId} content={desc}>
            {label}
          </Hint>
        ) : (
          <>
            <span id={labelId}>{label}</span>
            {desc && <div className="ol-setting-row-description">{desc}</div>}
          </>
        )}
      </div>
      <div className="ol-setting-row-control ol-flex-row">
        <SettingLabel.Provider value={labelId}>{children}</SettingLabel.Provider>
      </div>
    </div>
  );
}

export function Toggle({
  on,
  onToggle,
  disabled = false,
  label,
  variant = 'default',
}: {
  on: boolean;
  onToggle?: (next: boolean) => void;
  disabled?: boolean;
  label?: string;
  variant?: 'default' | 'flat';
}) {
  const labelId = useContext(SettingLabel);
  return (
    <Switch.Root
      className="ol-switch"
      data-variant={variant}
      checked={on}
      onCheckedChange={(next) => onToggle?.(next)}
      aria-label={label}
      aria-labelledby={label ? undefined : labelId}
      disabled={disabled}
    >
      {/* Geometry, colours and motion live in .ol-switch (refinements.css). Width and
          height are locked there, so a column-direction row can't stretch it into a ball. */}
      <Switch.Thumb className="ol-switch-thumb" />
    </Switch.Root>
  );
}
