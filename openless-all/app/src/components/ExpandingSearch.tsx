import { useEffect, useRef, useState } from 'react';
import { Icon } from './Icon';

interface ExpandingSearchProps {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  label: string;
}

/** Circular magnifier that expands left into a field. Closed press scales like a seek control. */
export function ExpandingSearch({ value, onChange, placeholder, label }: ExpandingSearchProps) {
  const [open, setOpen] = useState(value.trim().length > 0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (value.trim()) setOpen(true);
  }, [value]);

  return (
    <div className={open ? 'ol-search ol-search-open' : 'ol-search'}>
      <input
        ref={inputRef}
        className="ol-search-field"
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
    </div>
  );
}
