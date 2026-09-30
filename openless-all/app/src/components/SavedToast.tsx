import { AnimatePresence, motion } from 'framer-motion';
import type { CSSProperties } from 'react';
import { useEffect, useState } from 'react';
import { useReducedMotion } from '../lib/motion';
import { Icon } from './Icon';

const TOAST_TRANSITION = { type: 'spring' as const, damping: 26, stiffness: 320 };

export type SaveToastState = 'idle' | 'saving' | 'saved' | 'failed';

// Enter/exit direction — always "leave the way you came" (same direction both ways).
//   'right': slides in from the right edge and back out right — page-level toasts
//   (marketplace / translation / selection ask …).
//   'top'  : slides in from the top and back out top — toasts inside the settings dialog.
export type ToastSlideFrom = 'right' | 'top';

interface SavedToastProps {
  saveState: SaveToastState;
  message: string;
  offsetStyle?: Pick<CSSProperties, 'top' | 'right' | 'left' | 'bottom' | 'position'>;
  slideFrom?: ToastSlideFrom;
  actionLabel?: string;
  onAction?: () => void;
  durationMs?: number;
}

export function SavedToast({
  saveState,
  message,
  offsetStyle,
  slideFrom = 'right',
  actionLabel,
  onAction,
  durationMs,
}: SavedToastProps) {
  // Internal state lets the toast time itself out (even if the parent's timer runs
  // longer than 0.8s).
  const [internalVisible, setInternalVisible] = useState(false);
  const reduced = useReducedMotion();

  useEffect(() => {
    if (saveState !== 'idle') {
      setInternalVisible(true);
      if (saveState === 'saving') return;
      const timer = window.setTimeout(
        () => setInternalVisible(false),
        durationMs ?? (onAction ? 6000 : 800),
      );
      return () => window.clearTimeout(timer);
    }
    setInternalVisible(false);
  }, [saveState, message, durationMs, onAction]);

  const failed = saveState === 'failed';

  // Docked top-right — same zone as the "marketplace / refresh / import ZIP" header
  // buttons. position:fixed anchors to the viewport: slide in/out hugs the screen
  // edge and can't stretch the page into a scrollbar. The settings dialog passes its
  // own offsetStyle to switch to absolute (anchored to the dialog content's corner).
  const style: CSSProperties = {
    position: 'fixed',
    top: 20,
    right: 28,
    ...offsetStyle,
    zIndex: 99999,
    padding: '4px 11px',
    borderRadius: 999,
    border: failed ? '0.5px solid rgba(239,68,68,0.22)' : '0.5px solid rgba(37,99,235,0.16)',
    background: failed ? '#fef2f2' : '#eff4ff',
    color: failed ? '#dc2626' : '#2563eb',
    fontSize: 11.5,
    fontWeight: 600,
    lineHeight: 1.5,
    boxShadow: failed
      ? '0 4px 12px -8px rgba(239,68,68,.28)'
      : '0 4px 12px -8px rgba(37,99,235,.26)',
    backdropFilter: 'blur(12px) saturate(160%)',
    WebkitBackdropFilter: 'blur(12px) saturate(160%)',
    pointerEvents: onAction ? 'auto' : 'none',
    whiteSpace: 'nowrap',
    display: 'flex',
    alignItems: 'center',
    gap: 6,
  };

  // "Leave the way you came": enter start == exit end; direction comes from slideFrom.
  // Both branches spell out x / y so the motion variant stays complete — no axis
  // falling back to an implicit default.
  const offscreen = reduced
    ? { opacity: 0, x: 0, y: 0 }
    : slideFrom === 'top'
      ? { opacity: 0, x: 0, y: '-220%' }
      : { opacity: 0, x: '120%', y: 0 };

  return (
    <AnimatePresence>
      {internalVisible && (
        <motion.div
          role={failed ? 'alert' : 'status'}
          initial={offscreen}
          animate={{ opacity: 1, x: 0, y: 0 }}
          exit={offscreen}
          transition={reduced ? { duration: 0 } : TOAST_TRANSITION}
          style={style}
        >
          {saveState === 'saving' ? (
            <span className="ol-loading-spinner" aria-hidden="true">
              <Icon name="refresh" size={12} />
            </span>
          ) : failed ? (
            '⚠️'
          ) : (
            '✓'
          )}{' '}
          {message}
          {actionLabel && onAction && (
            <button
              type="button"
              onClick={onAction}
              style={{
                marginLeft: 4,
                border: 0,
                background: 'transparent',
                color: 'inherit',
                font: 'inherit',
                fontWeight: 700,
                textDecoration: 'underline',
                cursor: 'pointer',
              }}
            >
              {actionLabel}
            </button>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
