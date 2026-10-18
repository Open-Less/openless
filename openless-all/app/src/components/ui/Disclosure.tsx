import { useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Collapsible } from '@base-ui/react/collapsible';
import { Icon } from '../Icon';

export function Collapse({
  open,
  children,
  id,
}: {
  open: boolean;
  children: ReactNode;
  id?: string;
}) {
  // Preserve the outgoing content until its height/opacity transition finishes;
  // callers can clear the associated notice or operation state immediately.
  const retained = useRef(children);
  if (open) retained.current = children;
  return (
    <Collapsible.Root open={open} render={<div style={{ display: 'contents' }} />}>
      <Collapsible.Panel
        keepMounted
        className="ol-collapse"
        data-open={open}
        id={id}
        aria-hidden={!open}
        ref={(element) => {
          if (element) element.inert = !open;
        }}
      >
        <div>{open ? children : retained.current}</div>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}

/** The inner node is measured only on resize/content changes, never on animation frames. */
export function ExpandableText({ text, lines = 3 }: { text: string; lines?: number }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [height, setHeight] = useState({ full: 0, folded: 0 });
  const inner = useRef<HTMLParagraphElement>(null);
  const id = useId();
  useLayoutEffect(() => {
    const element = inner.current;
    if (!element) return;
    const measure = () => {
      const full = element.offsetHeight;
      const folded = Math.round(parseFloat(getComputedStyle(element).lineHeight) * lines);
      setHeight((previous) =>
        previous.full === full && previous.folded === folded ? previous : { full, folded },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [text, lines]);
  const expandable = height.full > height.folded + 1;
  return (
    <div className="ol-expandable-text">
      <div
        id={id}
        className="ol-expandable-text-clip"
        style={{
          height: height.full
            ? open
              ? height.full
              : Math.min(height.full, height.folded)
            : undefined,
        }}
      >
        <p ref={inner}>{text}</p>
      </div>
      {expandable && (
        <button
          type="button"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => setOpen(!open)}
        >
          {t(`uiRefinement.${open ? 'collapse' : 'expand'}`)}
          <Icon name="chevDown" size={12} style={{ transform: `rotate(${open ? 180 : 0}deg)` }} />
        </button>
      )}
    </div>
  );
}
