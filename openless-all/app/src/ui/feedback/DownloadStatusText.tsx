import { useLayoutEffect, useRef } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import type { DownloadStatusContent } from '../../api/interaction';
import { useReducedMotion } from '../../lib/motion';
import './download-status.css';

export function downloadPercentParts(percent: number, locale?: string) {
  const bounded = Number.isFinite(percent) ? Math.min(100, Math.max(0, percent)) : 0;
  const belowOne = bounded > 0 && bounded < 1;
  const parts = new Intl.NumberFormat(locale, { style: 'percent' }).formatToParts(
    (belowOne ? 1 : Math.floor(bounded)) / 100,
  );
  const numberIndex = parts.findIndex((part) => part.type === 'integer');
  const before = parts
    .slice(0, numberIndex)
    .map((part) => part.value)
    .join('');
  const digits = `${belowOne ? '<' : ''}${parts[numberIndex].value}`;
  const after = parts
    .slice(numberIndex + 1)
    .map((part) => part.value)
    .join('');
  return { before, digits, after, label: before + digits + after };
}

export function downloadStatusMessage(status: DownloadStatusContent) {
  const { beforeName, modelName = '', afterName = '', percent, locale } = status;
  const value = percent == null ? '' : ` · ${downloadPercentParts(percent, locale).label}`;
  return beforeName + modelName + afterName + value;
}

function PhaseText({ text }: { text: string }) {
  const reduced = useReducedMotion();
  if (reduced) return text;
  return (
    <AnimatePresence initial={false} mode="wait">
      <motion.span
        key={text}
        className="ol-download-phase-text"
        initial={{ opacity: 0, filter: 'blur(1.2px)' }}
        animate={{ opacity: 1, filter: 'none', transition: { duration: 0.12, ease: 'easeOut' } }}
        exit={{ opacity: 0, filter: 'blur(1.2px)', transition: { duration: 0.12, ease: 'easeIn' } }}
      >
        {text}
      </motion.span>
    </AnimatePresence>
  );
}

function RollingDownloadPercent({ percent, locale }: { percent: number; locale?: string }) {
  const { before, digits, after } = downloadPercentParts(percent, locale);
  const reduced = useReducedMotion();
  const track = useRef<HTMLSpanElement>(null);
  const outgoing = useRef<HTMLSpanElement>(null);
  const previous = useRef(digits);

  useLayoutEffect(() => {
    const prior = previous.current;
    previous.current = digits;
    const node = track.current;
    const ghost = outgoing.current;
    if (reduced || prior === digits || !node?.animate || !ghost) return;
    ghost.dataset.previous = prior;
    // Both values are fixed one line apart. One animation moves the entire reel,
    // including inside the notification's smaller inherited line-height.
    const animation = node.animate(
      [{ transform: 'translateY(100%)' }, { transform: 'translateY(0)' }],
      { duration: 180, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' },
    );
    return () => animation.cancel();
  }, [digits, reduced]);

  return (
    <span className="ol-download-percent">
      {before}
      <span className="ol-download-digits">
        <span ref={track} className="ol-download-digits-track">
          <span ref={outgoing} className="ol-download-digits-outgoing" aria-hidden="true" />
          <span className="ol-download-digits-current">{digits}</span>
        </span>
      </span>
      {after}
    </span>
  );
}

/** Only the phase wording fades. The model and percentage have their own stable slots. */
export function DownloadStatusText({ status }: { status: DownloadStatusContent }) {
  const { beforeName, modelName, afterName = '', percent, locale } = status;
  return (
    <>
      <PhaseText text={beforeName} />
      {modelName && <span className="ol-download-model-name">{modelName}</span>}
      <PhaseText text={afterName} />
      {percent !== undefined && (
        <span
          className="ol-download-progress-text"
          data-reserved-text={
            percent === null ? ` · ${downloadPercentParts(100, locale).label}` : undefined
          }
        >
          {percent === null ? null : (
            <>
              {' · '}
              <RollingDownloadPercent percent={percent} locale={locale} />
            </>
          )}
        </span>
      )}
    </>
  );
}
