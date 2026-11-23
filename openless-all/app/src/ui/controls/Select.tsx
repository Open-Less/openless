import { Select as BaseSelect, type SelectRootActions } from '@base-ui/react/select';
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useAppViewport, usePopupViewport } from '../layout/AppViewport';

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

interface OpenRequest { open: boolean }
interface OpenIntent {
  event: Event;
  accepted?: OpenRequest;
  release?: () => void;
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
  const { modalStack } = useAppViewport();
  // Base UI defers a mouse-triggered opening until the next animation frame.
  // Escape during that frame still belongs to this menu, not the parent modal.
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  const actions = useRef<SelectRootActions | null>(null);
  // Root is controlled by exactly one effective value. A fresh request object
  // also commits a refused controlled request so its transition can be released.
  const [request, setRequest] = useState<OpenRequest>({ open: false });
  const effectiveOpen = open ?? request.open;
  const config = useRef({ open, onOpenChange });
  config.current = { open, onOpenChange };
  const intent = useRef<OpenIntent | null>(null);
  const cancelled = useRef(new WeakSet<Event>());
  const cancelledClose = useRef<OpenRequest | null>(null);

  const retire = (owner: OpenIntent, close: boolean, focus = false) => {
    if (intent.current !== owner) return false;
    intent.current = null;
    cancelled.current.add(owner.event);
    owner.release?.();
    if (close && owner.accepted) {
      const update = { open: false };
      cancelledClose.current = update;
      setRequest(update);
      config.current.onOpenChange?.(false);
    }
    if (focus && config.current.open !== true) triggerRef.current?.focus({ preventScroll: true });
    return true;
  };

  useLayoutEffect(() => {
    if (disabled && intent.current) retire(intent.current, true);
  }, [disabled]);
  useLayoutEffect(() => {
    if (cancelledClose.current !== request) return;
    cancelledClose.current = null;
    // A revoked opening can commit closed before Popup mounts its transition
    // ref. Complete that cancelled mount through the public unmount action;
    // never unmount a controlled Root that its parent still keeps open.
    if (!effectiveOpen) actions.current?.unmount();
  }, [request, effectiveOpen]);
  const host = useRef({ modalStack, container });
  useLayoutEffect(() => {
    if ((host.current.modalStack !== modalStack || host.current.container !== container) && intent.current) {
      retire(intent.current, true);
    }
    host.current = { modalStack, container };
  }, [modalStack, container]);
  useLayoutEffect(() => () => {
    const owner = intent.current;
    if (owner) retire(owner, false);
  }, []);
  useEffect(() => {
    const owner = intent.current;
    if (owner?.accepted !== request) return;
    // The matching controlled Root render and its child dismissal effects have
    // committed before this parent's passive effect. Ordinary Escape is now
    // Base UI's responsibility (or the external controller refused to open).
    intent.current = null;
    if (!effectiveOpen) cancelled.current.add(owner.event);
    owner.release?.();
  });
  return (
    <BaseSelect.Root
      actionsRef={actions}
      open={effectiveOpen}
      value={value}
      items={options}
      disabled={disabled}
      modal={false}
      onOpenChange={(next, details) => {
        if (details.event && cancelled.current.has(details.event)) {
          details.cancel();
          return;
        }
        const update = { open: next };
        const owner = intent.current;
        if (owner) {
          if (next && details.event === owner.event) owner.accepted = update;
          else retire(owner, false);
        }
        setRequest(update);
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
          if (event.button === 0 && !disabled && !effectiveOpen) {
            if (intent.current) retire(intent.current, true);
            const owner: OpenIntent = { event: event.nativeEvent };
            intent.current = owner;
            const release = modalStack.claimPopupTransition({
              trigger: event.currentTarget,
              container,
              popup: () => popupRef.current,
              escape: () => retire(owner, true, true),
              invalidate: () => { retire(owner, true); },
            });
            if (!release) { retire(owner, false); return; }
            owner.release = release;
            // Safari does not consistently focus buttons on mouse press. Establish
            // the trigger before Base UI's deferred opening captures its focus origin.
            event.currentTarget.focus({ preventScroll: true });
          }
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
            ref={popupRef}
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
