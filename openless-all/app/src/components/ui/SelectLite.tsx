// Compatibility facade: every dropdown uses Base UI rather than maintaining a
// second positioning, keyboard, focus, scroll and exit-animation implementation.
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import { Combobox } from '@base-ui/react/combobox';
import { Icon } from '../Icon';
import { Select } from './Select';

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** Stays visible during search, e.g. a custom-input entry. */
  alwaysVisible?: boolean;
  trailing?: ReactNode;
}

interface SelectLiteProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  placeholder?: string;
  disabled?: boolean;
  style?: CSSProperties;
  ariaLabel?: string;
  onOpenChange?: (open: boolean) => void;
  searchable?: boolean;
  searchPlaceholder?: string;
  emptyMessage?: string;
}

export function SelectLite({
  value,
  onChange,
  options,
  placeholder,
  disabled,
  style,
  ariaLabel,
  onOpenChange,
  searchable = false,
  searchPlaceholder = 'Search…',
  emptyMessage = 'No matching options',
}: SelectLiteProps) {
  const open = useRef(false);
  const onOpenChangeRef = useRef(onOpenChange);
  onOpenChangeRef.current = onOpenChange;
  const handleOpenChange = useCallback((next: boolean) => {
    open.current = next;
    onOpenChangeRef.current?.(next);
  }, []);
  useEffect(
    () => () => {
      // In particular, release microphone metering when its settings page is left.
      if (open.current) onOpenChangeRef.current?.(false);
    },
    [],
  );

  if (searchable) {
    return (
      <SearchableSelect
        {...{
          value,
          onChange,
          options,
          placeholder,
          disabled,
          style,
          ariaLabel,
          searchPlaceholder,
          emptyMessage,
        }}
        onOpenChange={handleOpenChange}
      />
    );
  }
  return (
    <Select
      {...{ value, onChange, options, placeholder, disabled, style, ariaLabel }}
      onOpenChange={handleOpenChange}
    />
  );
}

function SearchableSelect({
  value,
  onChange,
  options,
  placeholder,
  disabled,
  style,
  ariaLabel,
  onOpenChange,
  searchPlaceholder,
  emptyMessage,
}: SelectLiteProps) {
  const [query, setQuery] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const optionMap = new Map(options.map((option) => [option.value, option]));
  const optionMapRef = useRef(optionMap);
  optionMapRef.current = optionMap;
  // Parents may recreate options on every progress/level/preferences update.
  // Keep item identity tied to values, not those renders, so scrolling is never
  // forced back to the selected item while the user is browsing a long list.
  const signature = JSON.stringify(options.map((option) => option.value));
  const items = useMemo<string[]>(() => JSON.parse(signature), [signature]);
  const filter = useCallback((item: string, input: string) => {
    const option = optionMapRef.current.get(item);
    const needle = input.trim().toLocaleLowerCase();
    return (
      !!option &&
      (option.alwaysVisible === true ||
        option.label.toLocaleLowerCase().includes(needle) ||
        item.toLocaleLowerCase().includes(needle))
    );
  }, []);
  const selected = optionMap.get(value);
  return (
    <Combobox.Root<string>
      items={items}
      value={value}
      disabled={disabled}
      modal={false}
      filter={filter}
      inputValue={query}
      onInputValueChange={setQuery}
      itemToStringLabel={(item) => optionMapRef.current.get(item)?.label ?? item}
      onOpenChange={(next) => {
        if (next) setQuery('');
        onOpenChange?.(next);
      }}
      onValueChange={(next) => {
        if (next !== null) onChange(next);
      }}
    >
      <Combobox.Trigger className="ol-select-trigger" style={style} aria-label={ariaLabel}>
        <span className="ol-select-value">{selected?.label ?? placeholder}</span>
        <Combobox.Icon>
          <Icon name="chevDown" size={11} />
        </Combobox.Icon>
      </Combobox.Trigger>
      <Combobox.Portal className="ol-popup-portal">
        <Combobox.Positioner
          className="ol-select-positioner"
          positionMethod="fixed"
          side="bottom"
          collisionAvoidance={{ side: 'flip', align: 'shift', fallbackAxisSide: 'none' }}
          align="start"
          sideOffset={8}
          collisionPadding={8}
        >
          <Combobox.Popup className="ol-select-popup" initialFocus={inputRef}>
            <div className="ol-select-search">
              <Icon name="search" size={13} />
              <Combobox.Input
                ref={inputRef}
                placeholder={searchPlaceholder}
                aria-label={searchPlaceholder}
              />
            </div>
            <Combobox.Empty className="ol-select-empty">{emptyMessage}</Combobox.Empty>
            <Combobox.List className="ol-select-list">
              {(item: string) => {
                const option = optionMap.get(item);
                if (!option) return null;
                return (
                  <Combobox.Item
                    key={item}
                    value={item}
                    disabled={option.disabled}
                    className="ol-select-item"
                  >
                    <span>{option.label}</span>
                    {option.trailing && (
                      <span className="ol-select-trailing">{option.trailing}</span>
                    )}
                    <Combobox.ItemIndicator className="ol-select-check">
                      <Icon name="check" size={12} />
                    </Combobox.ItemIndicator>
                  </Combobox.Item>
                );
              }}
            </Combobox.List>
          </Combobox.Popup>
        </Combobox.Positioner>
      </Combobox.Portal>
    </Combobox.Root>
  );
}
