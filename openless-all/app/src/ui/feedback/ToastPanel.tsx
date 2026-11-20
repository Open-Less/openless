/**
 * Glyphs, tile motion, and continuous-corner panel adapted from Rare UI's Toast.
 * Copyright (c) 2026 Swami Malode. See public/licenses/rare-ui-LICENSE.txt.
 * https://www.rareui.com/components/toast
 */
import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { getSvgPath } from 'figma-squircle';
import type { ToastVariant } from '../../api/interaction';

const PADDING = 10;
const TILE = 36;
const TILE_RADIUS = 11;
const CARD_RADIUS = TILE_RADIUS + PADDING;
const squircle = (width: number, height: number, cornerRadius: number) =>
  getSvgPath({ width, height, cornerRadius, cornerSmoothing: 1 });
const TILE_CLIP = `path('${squircle(TILE, TILE, TILE_RADIUS)}')`;

const POP = { type: 'spring', stiffness: 520, damping: 30 } as const;
// critically damped, so the tile eases in without the glyph's bounce
const SETTLE = { type: 'spring', stiffness: 320, damping: 36 } as const;
// The viewport entrance is 220 ms; reveal the glyph after the shell starts to settle.
const ARRIVAL = 0.16;
const INSTANT = { duration: 0 } as const;
// a promise result waits for the spinner to leave, then the tile gives one small bump
const LEAVE = { duration: 0.14, ease: 'easeIn' } as const;
const BUMP = {
  duration: 0.4,
  delay: LEAVE.duration,
  ease: 'easeOut' as const,
  times: [0, 0.4, 1],
};

function Glyph({ type, delay }: { type: ToastVariant; delay: number }) {
  // Use real SVG user-unit distances; WebKit can leave normalized pathLength dashes split.
  const stroke = (length: number) =>
    ({
      '--ol-toast-stroke-dashes': `${length} ${length + 4}`,
      '--ol-toast-stroke-start': length + 2,
      animationDelay: `${40 + delay * 1000}ms`,
    }) as CSSProperties;

  if (type === 'loading') {
    return (
      <svg
        viewBox="0 0 24 24"
        className="ol-toast-spinner"
        width={18}
        height={18}
        fill="none"
        stroke="currentColor"
        strokeWidth={2.75}
        strokeLinecap="round"
      >
        <circle cx="12" cy="12" r="8" strokeOpacity={0.25} />
        <path d="M12 4a8 8 0 0 1 8 8" />
      </svg>
    );
  }

  return (
    <svg
      viewBox="0 0 24 24"
      width={18}
      height={18}
      fill="none"
      stroke="currentColor"
      strokeWidth={3}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {type === 'success' && (
        <path className="ol-toast-stroke" d="M6 12.5l4 4 8-9" style={stroke(18)} />
      )}
      {type === 'trash' && (
        <path
          d="M4.5 6.5h15m-10.5 0V4h6v2.5M6.5 6.5l1 13.5h9l1-13.5M10 10v6m4-6v6"
          strokeWidth={2}
        />
      )}
      {type === 'error' && (
        <>
          <path className="ol-toast-stroke" d="M7.5 7.5l9 9" style={stroke(13)} />
          <path className="ol-toast-stroke" d="M16.5 7.5l-9 9" style={stroke(13)} />
        </>
      )}
      {type === 'warning' && (
        <>
          <path className="ol-toast-stroke" d="M12 6v7" style={stroke(8)} />
          <circle cx="12" cy="18" r="1.6" fill="currentColor" stroke="none" />
        </>
      )}
      {type === 'info' && (
        <>
          <circle cx="12" cy="6.25" r="1.6" fill="currentColor" stroke="none" />
          <path className="ol-toast-stroke" d="M12 11v7" style={stroke(8)} />
        </>
      )}
    </svg>
  );
}

export function ToastIcon({ type }: { type: Exclude<ToastVariant, 'default'> }) {
  const reduced = useReducedMotion() ?? false;
  // only the first icon waits for the toast to land; a promise swap animates right away
  const [arrivingType] = useState(type);
  const settled = type !== arrivingType;
  const delay = settled ? 0 : ARRIVAL;
  const success = type === 'success';
  const bump = settled && !success;

  return (
    <motion.span
      aria-hidden
      data-slot="toast-icon"
      initial={reduced ? false : { scale: 0.85, opacity: 0 }}
      animate={{ scale: bump ? [1, 1.08, 1] : 1, opacity: 1 }}
      transition={
        reduced
          ? INSTANT
          : bump
            ? { scale: BUMP }
            : { ...SETTLE, delay: settled ? 0 : ARRIVAL - 0.08 }
      }
      style={{ width: TILE, height: TILE, clipPath: TILE_CLIP }}
      className="ol-toast-notification-icon"
      data-tone={type}
    >
      <AnimatePresence initial={!reduced} mode="wait">
        <motion.span
          key={type}
          initial={{ scale: success ? 1 : 0.4, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{
            scale: 0.5,
            opacity: 0,
            transition: reduced ? INSTANT : LEAVE,
          }}
          transition={reduced ? INSTANT : success ? { duration: 0.12, delay } : { ...POP, delay }}
          className="ol-toast-glyph"
        >
          <Glyph type={type} delay={delay} />
        </motion.span>
      </AnimatePresence>
    </motion.span>
  );
}

export function ToastPanel({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [{ width, height }, setSize] = useState({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => {
      // Local box dimensions remain valid inside a scaled viewport or an entering toast.
      const { offsetWidth, offsetHeight } = element;
      setSize((size) =>
        size.width === offsetWidth && size.height === offsetHeight
          ? size
          : { width: offsetWidth, height: offsetHeight },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const outline = width > 0 && height > 0 ? squircle(width, height, CARD_RADIUS) : null;
  return (
    <div
      className="ol-toast-panel"
      ref={ref}
      style={{
        padding: PADDING,
        borderRadius: CARD_RADIUS,
        clipPath: outline ? `path('${outline}')` : undefined,
      }}
    >
      {outline && (
        <svg aria-hidden="true" width={width} height={height} className="ol-toast-outline">
          <path d={outline} fill="none" strokeWidth={2} />
        </svg>
      )}
      {children}
    </div>
  );
}
