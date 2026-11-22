import { Select as BaseSelect } from '@base-ui/react/select';
import { useRef, type CSSProperties, type ReactNode } from 'react';
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
  // Base UI defers a mouse-triggered opening until the next animation frame.
  // Escape during that frame still belongs to this menu, not the parent modal.
  const triggerRef = useRef<HTMLButtonElement>(null);
  const pendingOpen = useRef<Event | null>(null);
  const cancelledOpen = useRef<Event | null>(null);
  return (
    <BaseSelect.Root
      open={open}
      value={value}
      items={options}
      disabled={disabled}
      modal={false}
      onOpenChange={(next, details) => {
        if (details.event === pendingOpen.current) pendingOpen.current = null;
        if (next && details.event === cancelledOpen.current) {
          cancelledOpen.current = null;
          details.cancel();
          return;
        }
        onOpenChange?.(next);
      }}
      onOpenChangeComplete={onOpenChangeComplete}
      onValueChange={(next) => {
        if (next !== null) onChange(next);
      }}
    >
      <BaseSelect.Trigger
        ref={triggerRef}
        className="ol-select-trigger"
        aria-label={ariaLabel}
        style={style}
        onMouseDownCapture={(event) => {
          if (event.button === 0 && !disabled && event.currentTarget.getAttribute('aria-expanded') !== 'true') {
            // Safari does not consistently focus buttons on mouse press. Establish
            // the trigger before Base UI's deferred opening captures its focus origin.
            event.currentTarget.focus({ preventScroll: true });
            pendingOpen.current = event.nativeEvent;
          }
        }}
        onKeyDownCapture={(event) => {
          if (event.key !== 'Escape' || event.nativeEvent.isComposing || !pendingOpen.current) return;
          cancelledOpen.current = pendingOpen.current;
          pendingOpen.current = null;
          event.preventDefault();
          event.stopPropagation();
        }}
      >
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
            finalFocus={(interaction) => interaction === 'keyboard' ? triggerRef.current : true}
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
