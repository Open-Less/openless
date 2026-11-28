import { useEffect, useRef, useState } from 'react';
import { Icon } from './Icon';
import { FieldFrame } from '../ui/controls/TextField';

interface ExpandingSearchProps {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  label: string;
  expandTo?: 'left' | 'right';
}

/** Keep the magnifier on the anchored side while the input grows. */
export function ExpandingSearch({
  value,
  onChange,
  placeholder,
  label,
  expandTo = 'left',
}: ExpandingSearchProps) {
  const [open, setOpen] = useState(value.trim().length > 0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (value.trim()) setOpen(true);
  }, [value]);

  const input = (
    <input
      ref={inputRef}
      className="ol-field-control ol-search-field"
      type="text"
      value={value}
      placeholder={placeholder}
      aria-label={label}
      tabIndex={open ? 0 : -1}
      onChange={(event) => onChange(event.target.value)}
      onBlur={() => {
        if (!value.trim()) setOpen(false);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          onChange('');
          setOpen(false);
        }
      }}
    />
  );
  const trigger = (
    <button
      type="button"
      className="ol-search-icon"
      aria-label={label}
      aria-expanded={open}
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => {
        if (open && value.trim()) {
          onChange('');
          inputRef.current?.focus();
          return;
        }
        const next = !open;
        setOpen(next);
        if (next) {
          window.setTimeout(() => inputRef.current?.focus(), 60);
        } else if (document.activeElement instanceof HTMLElement) {
          document.activeElement.blur();
        }
      }}
    >
      <Icon name="search" size={15} />
    </button>
  );

  return (
    <FieldFrame
      className={[
        'ol-search',
        open && 'ol-search-open',
        expandTo === 'right' && 'ol-search-expand-right',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      {expandTo === 'right' && trigger}
      {input}
      {expandTo === 'left' && trigger}
    </FieldFrame>
  );
}
