import { useId, useRef, useState, type ReactElement, type ReactNode } from 'react';
import { Tooltip as BaseTooltip } from '@base-ui/react/tooltip';

export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <BaseTooltip.Provider delay={200} timeout={400}>
      {children}
    </BaseTooltip.Provider>
  );
}

interface TooltipProps {
  content: ReactNode;
  placement?: 'right' | 'top' | 'bottom';
  wrap?: boolean;
  focusable?: boolean;
  children: ReactNode;
  id?: string;
  className?: string;
  trigger?: ReactElement;
}

/** Base UI owns focus, Escape, outside dismissal, collision handling and portal lifetime. */
export function Tooltip({
  content,
  placement = 'right',
  wrap = false,
  focusable = false,
  children,
  id,
  className,
  trigger,
}: TooltipProps) {
  const generatedId = useId();
  const triggerId = id ?? generatedId;
  const [open, setOpen] = useState(false);
  const explicitOpen = useRef(false);
  return (
    <BaseTooltip.Root
      open={open}
      triggerId={triggerId}
      onOpenChange={(next, details) => {
        // A tap/keyboard focus isn't a hover. Synthetic mouse-leave events must not
        // immediately dismiss it; outside press, blur and Escape still do.
        if (!next && details.reason === 'trigger-hover' && explicitOpen.current) {
          details.cancel();
          return;
        }
        if (!next) explicitOpen.current = false;
        setOpen(next);
      }}
    >
      <BaseTooltip.Trigger
        id={triggerId}
        tabIndex={focusable ? 0 : undefined}
        aria-describedby={open ? `${generatedId}-description` : undefined}
        render={trigger ?? (focusable ? <button type="button" /> : <span />)}
        className={['ol-tooltip-anchor', className].filter(Boolean).join(' ')}
        delay={200}
        closeOnClick={!focusable}
        // A real button makes explanatory text available to touch and keyboard users, too.
        onClick={
          focusable
            ? () => {
                explicitOpen.current = true;
                setOpen(true);
              }
            : undefined
        }
        onBlur={() => {
          explicitOpen.current = false;
        }}
        onFocus={
          focusable
            ? (event) => {
                // Re-entering by keyboard should work even after Escape dismissed a hover-only hint.
                if (event.currentTarget.matches(':focus-visible')) {
                  explicitOpen.current = true;
                  setOpen(true);
                }
              }
            : undefined
        }
      >
        {children}
      </BaseTooltip.Trigger>
      <BaseTooltip.Portal className="ol-popup-portal">
        <BaseTooltip.Positioner
          className="ol-tooltip-positioner"
          positionMethod="fixed"
          side={placement}
          sideOffset={8}
          collisionPadding={8}
        >
          <BaseTooltip.Popup
            id={`${generatedId}-description`}
            role="tooltip"
            className="ol-tooltip-bubble"
            data-wrap={wrap || undefined}
          >
            {content}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

/** The shared explanatory-label pattern: dotted words, no extra help icon. */
export function Hint({
  children,
  content,
  id,
}: {
  children: ReactNode;
  content: ReactNode;
  id?: string;
}) {
  return (
    <Tooltip id={id} content={content} placement="right" wrap focusable className="ol-hint-trigger">
      {children}
    </Tooltip>
  );
}
