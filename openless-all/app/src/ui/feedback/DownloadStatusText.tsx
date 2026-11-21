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
  const live = useRef<HTMLSpanElement>(null);
  const outgoing = useRef<HTMLSpanElement>(null);
  const previous = useRef({ digits, percent });
  const animations = useRef<Animation[]>([]);

  useLayoutEffect(() => {
    const prior = previous.current;
    previous.current = { digits, percent };
    if (prior.digits === digits && !reduced) return;
    const interrupted = animations.current.some((animation) => animation.playState === 'running');
    animations.current.forEach((animation) => animation.cancel());
    animations.current = [];

    const current = live.current;
    const ghost = outgoing.current;
    // A fast tick replaces the in-flight roll with the latest readable value.
    if (reduced || interrupted || !current?.animate || !ghost || prior.digits === digits) return;
    ghost.dataset.previous = prior.digits;
    const distance = percent >= prior.percent ? 100 : -100;
    const timing = { duration: 180, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' };
    animations.current = [
      ghost.animate(
        [
          { opacity: 1, transform: 'translateY(0)' },
          { opacity: 1, transform: `translateY(${-distance}%)` },
        ],
        timing,
      ),
      current.animate(
        [{ transform: `translateY(${distance}%)` }, { transform: 'translateY(0)' }],
        timing,
      ),
    ];
  }, [digits, percent, reduced]);

  useLayoutEffect(
    () => () => {
      animations.current.forEach((animation) => animation.cancel());
    },
    [],
  );

  return (
    <span className="ol-download-percent">
      {before}
      <span className="ol-download-digits">
        <span ref={outgoing} className="ol-download-digits-outgoing" aria-hidden="true" />
        <span ref={live} className="ol-download-digits-current">
          {digits}
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
