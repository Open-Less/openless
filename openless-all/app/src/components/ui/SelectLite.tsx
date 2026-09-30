// SelectLite — custom dropdown unified across the three platforms, replacing native
// <select> to avoid the ugly square Win32 ComboBox frame (issue #418) and the visual
// mismatch of the native NSPopUpButton on WKWebView.
//
// Design:
// - Trigger is a button (chevron + current value), style overridable via `style`
// - Popover renders into document.body via portal, dodging parent overflow:hidden
// - Keyboard: ArrowDown/ArrowUp move highlight, Enter confirms, Esc closes
// - Clicking or scrolling outside closes (scroll inside the popover doesn't)
// - Close plays a .14s exit animation
// - Second positioning runs via popoverMounted state + useLayoutEffect synchronously;
//   the whole round converges to the final anchor before paint, avoiding the
//   "paint at the fallback position, then paint again at the real one" flicker
// - CSS `zoom` compensation (critical): fontScale.ts scales the page via
//   `html.style.zoom`. WKWebView is inconsistent under zoom: getBoundingClientRect
//   returns post-zoom (visual) coords, while position:fixed left/top/width are treated
//   as pre-zoom (layout) coords. Setting `left=rect.left` directly renders the popover
//   at rect.left × zoom — off by rect.left × (zoom-1) px. Fix: divide visual coords by
//   zoom in setAnchor to get layout coords, so rendering ×zoom lands at the correct
//   visual position. See positionPopover.

import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { Icon } from '../Icon';

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** Stays visible during search, e.g. a custom-input entry. */
  alwaysVisible?: boolean;
  /** Optional: rendered right of the label, left of the check mark (e.g. mic volume bar). */
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
  /** Called on dropdown open/close — lets the caller start/stop side effects (e.g. level listening) per state. */
  onOpenChange?: (open: boolean) => void;
  /** Long lists can show a search box at the top of the popover; filtering matches label/value only, never mutates options. */
  searchable?: boolean;
  searchPlaceholder?: string;
  emptyMessage?: string;
}

const DEFAULT_TRIGGER_STYLE: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 8,
  padding: '0 10px',
  height: 32,
  fontSize: 12.5,
  fontFamily: 'inherit',
  borderRadius: 8,
  border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-select-trigger-bg)',
  color: 'var(--ol-ink)',
  cursor: 'default',
  outline: 'none',
  textAlign: 'left',
  minWidth: 160,
};

const EXIT_ANIM_MS = 140;

export function SelectLite({
  value,
  onChange,
  options,
  placeholder,
  disabled = false,
  style,
  ariaLabel,
  onOpenChange,
  searchable = false,
  searchPlaceholder = 'Search…',
  emptyMessage = 'No matching options',
}: SelectLiteProps) {
  const [open, setOpen] = useState(false);
  // leaving lets the popover finish its exit keyframe before unmount (reported as
  // "no shrink animation" — it used to unmount directly)
  const [leaving, setLeaving] = useState(false);
  const [highlight, setHighlight] = useState<number>(-1);
  const [query, setQuery] = useState('');
  const listboxId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const [anchor, setAnchor] = useState<{ left: number; top: number; width: number } | null>(null);
  // popoverMounted makes useLayoutEffect run positionPopover once more after the
  // popover actually enters the DOM, still before paint — see the useLayoutEffect
  // note below.
  const [popoverMounted, setPopoverMounted] = useState(false);

  const selected = useMemo(() => options.find((opt) => opt.value === value), [options, value]);
  const filteredOptions = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return options;
    return options.filter(
      (option) =>
        option.alwaysVisible ||
        option.label.toLocaleLowerCase().includes(needle) ||
        option.value.toLocaleLowerCase().includes(needle),
    );
  }, [options, query]);
  const displayLabel = selected?.label ?? placeholder ?? '';
  const highlightedOptionId = highlight >= 0 ? `${listboxId}-option-${highlight}` : undefined;

  const positionPopover = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    // Popover height only from real measurement; the 280 fallback applies only on the
    // first frame before mount.
    const popoverHeight = popoverRef.current?.getBoundingClientRect().height ?? 280;
    // Vertical: below the trigger by default; flip above when there isn't room,
    // avoiding viewport clipping.
    const spaceBelow = window.innerHeight - rect.bottom;
    const flipUp = spaceBelow < popoverHeight + 8 && rect.top > popoverHeight + 8;
    const visualTop = flipUp ? rect.top - popoverHeight - 4 : rect.bottom + 4;
    // The popover is forced to width=trigger.width (see style below), so maxLeft uses
    // rect.width; left stays identical across the unmounted/mounted frames, avoiding
    // a first-paint jump.
    const minLeft = 8;
    const maxLeft = Math.max(minLeft, window.innerWidth - rect.width - 8);
    const visualLeft = Math.min(Math.max(rect.left, minLeft), maxLeft);
    // ── CSS zoom compensation (root cause of the offset) ──
    // fontScale scales the page via `document.documentElement.style.zoom` (see fontScale.ts).
    // WKWebView is inconsistent under zoom: getBoundingClientRect returns post-zoom
    // (visual) coords, while position:fixed left/top/width are treated as pre-zoom
    // (layout) coords and multiplied by zoom at render. Setting left=rect.left directly
    // shifts the popover visually to rect.left × zoom (right by rect.left×(zoom-1)).
    // Divide the visual coords by zoom to get layout coords, so position:fixed renders
    // back at the correct visual position.
    const zoomStr = document.documentElement.style.zoom;
    const zoom = zoomStr ? parseFloat(zoomStr) || 1 : 1;
    setAnchor({
      left: visualLeft / zoom,
      top: visualTop / zoom,
      width: rect.width / zoom,
    });
  }, []);

  // Popover ref callback: runs on each popover DOM mount/unmount, only flips
  // popoverMounted. The actual second positioning is triggered by the useLayoutEffect
  // below via the popoverMounted dep — so the second positionPopover runs synchronously
  // before paint instead of in requestAnimationFrame (already after paint), avoiding
  // the double-paint flicker of "paint once at the 280 fallback, then again at the real
  // position" (the flipUp decision can flip between the 280 fallback and real height).
  const setPopoverRef = useCallback((node: HTMLDivElement | null) => {
    popoverRef.current = node;
    setPopoverMounted(!!node);
  }, []);

  // Both positioning phases run synchronously before paint:
  // 1) open false→true: popoverRef is still null, positionPopover sets an anchor using
  //    the 280 fallback so the portal condition `open && anchor` passes and the popover
  //    enters the DOM (avoiding the pre-v1.3.1-8 deadlock where anchor=null never rendered).
  // 2) popoverMounted false→true: popoverRef now points at the real DOM, positionPopover
  //    computes the final anchor from the real height. The whole
  //    commit→layoutEffect→re-commit→layoutEffect round finishes before browser paint,
  //    so the user sees a single frame at the final position with no flicker.
  useLayoutEffect(() => {
    if (!open) return;
    positionPopover();
  }, [open, popoverMounted, positionPopover]);

  // After ArrowUp/Down changes the highlight, scroll it into view — keyboard users can
  // follow the highlight on long dropdowns exceeding the 280 maxHeight.
  useEffect(() => {
    if (!open || highlight < 0) return;
    const target = popoverRef.current?.querySelector(
      `[data-option-index="${highlight}"]`,
    ) as HTMLElement | null;
    const scrollContainer = target?.parentElement;
    if (!target || !scrollContainer) return;
    const optionTop = target.offsetTop;
    const optionBottom = optionTop + target.offsetHeight;
    if (optionTop < scrollContainer.scrollTop) {
      scrollContainer.scrollTop = optionTop;
    } else if (optionBottom > scrollContainer.scrollTop + scrollContainer.clientHeight) {
      scrollContainer.scrollTop = optionBottom - scrollContainer.clientHeight;
    }
  }, [highlight, open]);

  useEffect(() => {
    if (!open || !searchable || !popoverMounted) return;
    searchRef.current?.focus();
  }, [open, popoverMounted, searchable]);

  useEffect(() => {
    if (!open) return;
    const selectedIndex = filteredOptions.findIndex(
      (option) => option.value === value && !option.disabled,
    );
    setHighlight(
      selectedIndex >= 0 ? selectedIndex : filteredOptions.findIndex((option) => !option.disabled),
    );
  }, [filteredOptions, open, value]);

  // Click/scroll outside → close. Scroll inside the popover stays open.
  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (triggerRef.current?.contains(target)) return;
      if (popoverRef.current?.contains(target)) return;
      closeMenu();
    };
    // Scrolling anywhere outside the popover (wheel or scroll event) → close.
    // Scrolling inside (long-list scroll) hits popover.contains(target) → stays open.
    const handleScrollOutside = (event: Event) => {
      const target = event.target as Node | null;
      if (target && popoverRef.current?.contains(target)) return;
      closeMenu();
    };
    // Stay open and re-anchor on window resize, so minor desktop window tweaks don't
    // drop the current filter.
    const handleResize = () => positionPopover();

    document.addEventListener('mousedown', handlePointerDown);
    window.addEventListener('scroll', handleScrollOutside, { capture: true, passive: true });
    window.addEventListener('wheel', handleScrollOutside, { capture: true, passive: true });
    window.addEventListener('resize', handleResize);
    return () => {
      document.removeEventListener('mousedown', handlePointerDown);
      window.removeEventListener('scroll', handleScrollOutside, true);
      window.removeEventListener('wheel', handleScrollOutside, true);
      window.removeEventListener('resize', handleResize);
    };
    // closeMenu is a stable reference (no React state deps), so it's omitted from deps.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, positionPopover]);

  const openMenu = () => {
    if (disabled) return;
    setQuery('');
    const initial = options.findIndex((opt) => opt.value === value && !opt.disabled);
    setHighlight(initial >= 0 ? initial : options.findIndex((opt) => !opt.disabled));
    setLeaving(false);
    setOpen(true);
    onOpenChange?.(true);
  };

  const closeMenu = () => {
    if (!open) return;
    onOpenChange?.(false);
    setLeaving(true);
    window.setTimeout(() => {
      setOpen(false);
      setLeaving(false);
      setHighlight(-1);
      setQuery('');
      setAnchor(null);
    }, EXIT_ANIM_MS);
  };

  const selectIndex = (index: number) => {
    if (disabled) return;
    const option = filteredOptions[index];
    if (!option || option.disabled) return;
    onChange(option.value);
    closeMenu();
    triggerRef.current?.focus();
  };

  const moveHighlight = (direction: 1 | -1) => {
    if (filteredOptions.length === 0) return;
    let next = highlight;
    for (let i = 0; i < filteredOptions.length; i += 1) {
      next = (next + direction + filteredOptions.length) % filteredOptions.length;
      if (!filteredOptions[next]?.disabled) {
        setHighlight(next);
        return;
      }
    }
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;
    if (!open) {
      if (
        event.key === 'ArrowDown' ||
        event.key === 'ArrowUp' ||
        event.key === 'Enter' ||
        event.key === ' '
      ) {
        event.preventDefault();
        openMenu();
      }
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      closeMenu();
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveHighlight(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveHighlight(-1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (highlight >= 0) selectIndex(highlight);
    } else if (event.key === 'Tab') {
      closeMenu();
    }
  };

  const moveFocusFromTrigger = (backward: boolean) => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const focusable = Array.from(
      document.querySelectorAll<HTMLElement>(
        'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"]), [contenteditable="true"]',
      ),
    ).filter(
      (element) => element.getClientRects().length > 0 && !popoverRef.current?.contains(element),
    );
    const index = focusable.indexOf(trigger);
    focusable[index + (backward ? -1 : 1)]?.focus();
  };

  const handleSearchKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      closeMenu();
      triggerRef.current?.focus();
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveHighlight(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveHighlight(-1);
    } else if (event.key === 'Enter' && highlight >= 0) {
      event.preventDefault();
      selectIndex(highlight);
    } else if (event.key === 'Tab') {
      event.preventDefault();
      closeMenu();
      moveFocusFromTrigger(event.shiftKey);
    }
  };

  const triggerStyle: CSSProperties = {
    ...DEFAULT_TRIGGER_STYLE,
    ...style,
    opacity: disabled ? 0.5 : 1,
    cursor: disabled ? 'not-allowed' : 'default',
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="ol-focus-ring"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listboxId : undefined}
        aria-activedescendant={open && !searchable ? highlightedOptionId : undefined}
        aria-disabled={disabled}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => (open ? closeMenu() : openMenu())}
        onKeyDown={handleKeyDown}
        style={triggerStyle}
      >
        <span
          key={displayLabel}
          style={{
            flex: 1,
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            color: selected ? 'var(--ol-ink)' : 'var(--ol-ink-4)',
            // Value-change animation: the key change remounts the span, replaying
            // ol-select-value-in (global.css). Fires once per value change, not a
            // looping animation.
            animation: 'ol-select-value-in .16s var(--ol-motion-quick)',
          }}
        >
          {displayLabel}
        </span>
        <Icon name="chevDown" size={11} />
      </button>
      {open &&
        anchor &&
        createPortal(
          <div
            ref={setPopoverRef}
            style={{
              position: 'fixed',
              left: anchor.left,
              top: anchor.top,
              // Locked to the trigger width (not minWidth) so content can't stretch the
              // popover beyond the trigger; long labels truncate via textOverflow:ellipsis.
              width: anchor.width,
              maxHeight: searchable ? 320 : 280,
              overflow: 'hidden',
              padding: 4,
              borderRadius: 10,
              border: '0.5px solid var(--ol-select-popover-border)',
              background: 'var(--ol-select-popover-bg)',
              backdropFilter: 'blur(20px) saturate(180%)',
              WebkitBackdropFilter: 'blur(20px) saturate(180%)',
              boxShadow: 'var(--ol-select-popover-shadow)',
              zIndex: 9999,
              fontFamily: 'inherit',
              fontSize: 12.5,
              animation: leaving
                ? 'ol-select-pop-out .14s cubic-bezier(.4,.0,.7,.2) forwards'
                : 'ol-select-pop .14s var(--ol-motion-quick) both',
              transformOrigin: 'top center',
            }}
          >
            {searchable && (
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 7,
                  margin: '1px 1px 4px',
                  padding: '0 9px',
                  height: 34,
                  borderRadius: 7,
                  border: '0.5px solid var(--ol-line-strong)',
                  background: 'var(--ol-select-trigger-bg)',
                  color: 'var(--ol-ink-4)',
                }}
              >
                <Icon name="search" size={13} />
                <input
                  ref={searchRef}
                  role="combobox"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  onKeyDown={handleSearchKeyDown}
                  placeholder={searchPlaceholder}
                  aria-label={searchPlaceholder}
                  aria-autocomplete="list"
                  aria-expanded={open}
                  aria-controls={listboxId}
                  aria-activedescendant={highlightedOptionId}
                  style={{
                    flex: 1,
                    minWidth: 0,
                    border: 0,
                    outline: 0,
                    padding: 0,
                    background: 'transparent',
                    color: 'var(--ol-ink)',
                    font: 'inherit',
                  }}
                />
                <span
                  style={{
                    fontSize: 10.5,
                    whiteSpace: 'nowrap',
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {filteredOptions.length}/{options.length}
                </span>
              </div>
            )}
            <div
              id={listboxId}
              role="listbox"
              style={{ maxHeight: searchable ? 274 : 272, overflowY: 'auto' }}
            >
              {!filteredOptions.some((option) => !option.alwaysVisible) && (
                <div
                  style={{
                    padding: '18px 10px',
                    textAlign: 'center',
                    color: 'var(--ol-ink-4)',
                    fontSize: 12,
                  }}
                >
                  {emptyMessage}
                </div>
              )}
              {filteredOptions.map((option, index) => {
                const isSelected = option.value === value;
                const isHighlighted = index === highlight;
                return (
                  <div
                    key={option.value || `__opt_${index}`}
                    id={`${listboxId}-option-${index}`}
                    data-option-index={index}
                    role="option"
                    aria-selected={isSelected}
                    aria-disabled={option.disabled}
                    onMouseEnter={() => !option.disabled && setHighlight(index)}
                    onMouseDown={(event) => {
                      event.preventDefault();
                      selectIndex(index);
                    }}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 8,
                      padding: '7px 10px',
                      borderRadius: 6,
                      cursor: option.disabled ? 'not-allowed' : 'default',
                      opacity: option.disabled ? 0.45 : 1,
                      background:
                        isHighlighted && !option.disabled
                          ? 'var(--ol-select-option-hover-bg)'
                          : 'transparent',
                      color: isSelected ? 'var(--ol-blue)' : 'var(--ol-ink)',
                      fontWeight: isSelected ? 600 : 500,
                      whiteSpace: 'nowrap',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      transition: 'background 0.10s var(--ol-motion-quick)',
                    }}
                  >
                    <span
                      style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}
                    >
                      {option.label}
                    </span>
                    {option.trailing}
                    {isSelected && <Icon name="check" size={12} />}
                  </div>
                );
              })}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}
