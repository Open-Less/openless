import { Select as BaseSelect } from '@base-ui/react/select';
import type { CSSProperties, ReactNode } from 'react';
import { Icon } from '../Icon';

interface SelectProps {
  value: string;
  onChange: (value: string) => void;
  options: readonly { value: string; label: string; disabled?: boolean; trailing?: ReactNode }[];
  ariaLabel?: string;
  disabled?: boolean;
  placeholder?: string;
  style?: CSSProperties;
  onOpenChange?: (open: boolean) => void;
}

/** Standard Base UI Select; keyboard navigation, typeahead and focus return stay upstream. */
export function Select({
  value,
  onChange,
  options,
  ariaLabel,
  disabled,
  placeholder,
  style,
  onOpenChange,
}: SelectProps) {
  return (
    <BaseSelect.Root
      value={value}
      items={options}
      disabled={disabled}
      modal={false}
      onOpenChange={onOpenChange}
      onValueChange={(next) => {
        if (next !== null) onChange(next);
      }}
    >
      <BaseSelect.Trigger className="ol-select-trigger" aria-label={ariaLabel} style={style}>
        <BaseSelect.Value className="ol-select-value" placeholder={placeholder} />
        <BaseSelect.Icon>
          <Icon name="chevDown" size={11} />
        </BaseSelect.Icon>
      </BaseSelect.Trigger>
      <BaseSelect.Portal className="ol-popup-portal">
        <BaseSelect.Positioner
          className="ol-select-positioner"
          positionMethod="fixed"
          side="bottom"
          collisionAvoidance={{ side: 'flip', align: 'shift', fallbackAxisSide: 'none' }}
          align="start"
          sideOffset={8}
          collisionPadding={8}
          alignItemWithTrigger={false}
        >
          <BaseSelect.Popup className="ol-select-popup">
            <BaseSelect.List className="ol-select-list">
              {options.map((option) => (
                <BaseSelect.Item
                  key={option.value}
                  value={option.value}
                  disabled={option.disabled}
                  className="ol-select-item"
                >
                  <BaseSelect.ItemText>{option.label}</BaseSelect.ItemText>
                  {option.trailing && <span className="ol-select-trailing">{option.trailing}</span>}
                  <BaseSelect.ItemIndicator className="ol-select-check">
                    <Icon name="check" size={12} />
                  </BaseSelect.ItemIndicator>
                </BaseSelect.Item>
              ))}
            </BaseSelect.List>
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}
