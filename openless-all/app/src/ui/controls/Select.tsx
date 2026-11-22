import { Select as BaseSelect } from '@base-ui/react/select';
import type { CSSProperties, ReactNode } from 'react';
import { usePopupViewport } from '../layout/AppViewport';

import { Icon } from '../../components/Icon';

interface SelectProps {
  value: string;
  onChange: (value: string) => void;
  options: readonly { value: string; label: string; disabled?: boolean; trailing?: ReactNode }[];
  ariaLabel?: string;
  disabled?: boolean;
  placeholder?: string;
  style?: CSSProperties;
  onOpenChange?: (open: boolean) => void;
  onOpenChangeComplete?: (open: boolean) => void;
  open?: boolean;
  side?: 'top' | 'bottom';
  popupStyle?: CSSProperties;
  popupClassName?: string;
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
  onOpenChangeComplete,
  open,
  side = 'bottom',
  popupStyle,
  popupClassName,
}: SelectProps) {
  const { boundary, container } = usePopupViewport();
  return (
    <BaseSelect.Root
      open={open}
      value={value}
      items={options}
      disabled={disabled}
      modal={false}
      onOpenChange={onOpenChange}
      onOpenChangeComplete={onOpenChangeComplete}
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
      <BaseSelect.Portal container={container} className="ol-popup-portal">
        <BaseSelect.Positioner
          className="ol-select-positioner"
          positionMethod="fixed"
          collisionBoundary={boundary}
          side={side}
          collisionAvoidance={{ side: 'flip', align: 'shift', fallbackAxisSide: 'none' }}
          align="start"
          sideOffset={8}
          collisionPadding={8}
          alignItemWithTrigger={false}
        >
          <BaseSelect.Popup
            className={['ol-select-popup', popupClassName].filter(Boolean).join(' ')}
            style={popupStyle}
          >
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
