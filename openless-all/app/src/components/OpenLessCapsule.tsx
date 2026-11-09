import { motion } from 'framer-motion';
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { VoiceOrbStage } from './VoiceOrbStage';
import { useReducedMotion } from '../lib/motion';
import type { OS } from './WindowChrome';
import type { CapsuleState, CapsuleStyle } from '../lib/types';
import './CapsuleStyles.css';
import './OpenLessCapsule.css';

// Measured from the 60 fps reference: 232×64 physical pixels at 2×.
const ENVELOPE = [0.22, 0.34, 0.48, 0.65, 0.82, 1, 0.82, 0.65, 0.48, 0.34, 0.22];
const MORPH = { duration: 0.22, ease: [0.22, 1, 0.36, 1] as const };
export const DEFAULT_CAPSULE_EXIT_MS = 180;

function useCapsulePresence(ref: RefObject<HTMLDivElement>, leaving: boolean, preview: boolean) {
  const reduced = useReducedMotion();
  const interrupted = useRef<Keyframe | null>(null);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    const visible = { opacity: 1, transform: 'none', filter: 'none' };
    const hidden = {
      opacity: 0,
      transform: `translateY(${leaving ? 5 : 6}px)`,
      filter: `blur(${leaving ? 3 : 4}px)`,
    };
    const target = leaving ? hidden : visible;
    const apply = () => {
      Object.assign(node.style, target, { willChange: '' });
    };
    if (preview || reduced || typeof node.animate !== 'function') {
      interrupted.current = null;
      apply();
      return;
    }
    const from = interrupted.current ?? (leaving ? visible : hidden);
    interrupted.current = null;
    node.style.willChange = 'opacity, transform, filter';
    const animation = node.animate([from, target], {
      duration: leaving ? DEFAULT_CAPSULE_EXIT_MS : 240,
      easing: 'cubic-bezier(0.22, 1, 0.36, 1)',
      fill: 'both',
    });
    animation.id = 'ol-capsule-presence';
    let cancelled = false;
    void animation.finished
      .then(() => {
        if (cancelled) return;
        apply();
        animation.cancel();
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      const painted = getComputedStyle(node);
      interrupted.current = {
        opacity: painted.opacity,
        transform: painted.transform,
        filter: painted.filter,
      };
      Object.assign(node.style, interrupted.current, { willChange: '' });
      animation.cancel();
    };
  }, [ref, leaving, preview, reduced]);
}

export function CapsuleWaveform({ level, active = true }: { level: number; active?: boolean }) {
  const root = useRef<HTMLSpanElement>(null);
  const input = useRef(level);
  input.current = level;
  const reduced = useReducedMotion();
  useEffect(() => {
    const bars = root.current?.children;
    if (!bars) return;
    let frame = 0;
    let previous = 0;
    let smoothed = 0;
    const draw = (now: number) => {
      const dt = previous ? Math.min(now - previous, 50) : 16;
      previous = now;
      const voice = active ? Math.min(1, Math.max(0, (input.current - 0.008) / 0.32)) : 0;
      smoothed += (voice - smoothed) * (1 - Math.exp(-dt / (voice > smoothed ? 35 : 90)));
      ENVELOPE.forEach((envelope, index) => {
        // The centre stays strongest; neighbouring bars do not move as one rigid icon.
        const ripple = reduced ? 1 : 0.86 + 0.14 * Math.sin(now / 85 + index * 0.8);
        (bars[index] as HTMLElement).style.height =
          `${1.5 + 14.5 * Math.pow(smoothed, 0.6) * envelope * ripple}px`;
      });
      if (active && !reduced) frame = requestAnimationFrame(draw);
    };
    draw(performance.now());
    return () => cancelAnimationFrame(frame);
  }, [active, reduced, reduced ? level : undefined]);
  return (
    <span ref={root} className="ol-default-waveform" aria-hidden="true">
      {ENVELOPE.map((_, index) => (
        <span key={index} />
      ))}
    </span>
  );
}

interface OpenLessPillProps {
  os?: OS;
  state: CapsuleState;
  level: number;
  insertedChars?: number;
  message?: string;
  operating?: boolean;
  translation?: boolean;
  warming?: boolean;
  leaving?: boolean;
  onCancel?: () => void;
  onConfirm?: () => void;
  preview?: boolean;
}

/** A persistent shell clips the outgoing controls as it contracts. Content never scales. */
export function OpenLessPill({
  state,
  level,
  insertedChars = 0,
  message,
  operating = false,
  translation = false,
  warming = false,
  leaving = false,
  onCancel,
  onConfirm,
  preview = false,
}: OpenLessPillProps) {
  const { t } = useTranslation();
  const reduced = useReducedMotion();
  const root = useRef<HTMLDivElement>(null);
  useCapsulePresence(root, leaving, preview);
  const recording = state === 'recording';
  const processing = state === 'transcribing' || state === 'polishing';
  const label = processing
    ? message || t(operating ? 'capsule.using' : 'capsule.thinking')
    : state === 'done'
      ? message || t('capsule.inserted', { count: insertedChars })
      : state === 'cancelled'
        ? t('capsule.cancelled')
        : message || t('capsule.error');
  const labelRef = useRef<HTMLSpanElement>(null);
  const [statusWidth, setStatusWidth] = useState(80);
  useLayoutEffect(() => {
    const element = labelRef.current;
    if (!element) return;
    const measure = () => setStatusWidth(Math.min(300, Math.max(80, element.scrollWidth + 28)));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [label]);
  return (
    <motion.div
      ref={root}
      className="ol-capsule-pill ol-default-capsule"
      data-recording={recording}
      data-warming={warming}
      data-processing={processing}
      data-translation={translation && (recording || warming)}
      data-error={state === 'error'}
      data-leaving={leaving}
      initial={preview || reduced ? false : { width: 68 }}
      animate={{
        width: leaving || warming ? 68 : recording ? 116 : statusWidth,
      }}
      transition={reduced ? { duration: 0 } : MORPH}
    >
      <div className="ol-default-recording" aria-hidden={!recording && !warming}>
        <button
          className="ol-default-action ol-default-cancel"
          aria-label={t('common.cancel')}
          aria-hidden={!recording}
          disabled={!recording || preview}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onCancel}
        >
          <Icon name="close" size={15} strokeWidth={1.8} />
        </button>
        <CapsuleWaveform level={level} active={recording && !leaving} />
        <button
          className="ol-default-action ol-default-confirm"
          aria-label={t('settings.shortcuts.confirm')}
          aria-hidden={!recording}
          disabled={!recording || preview}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onConfirm}
        >
          <Icon name="check" size={17} strokeWidth={2} />
        </button>
      </div>
      <div className="ol-default-status" aria-hidden={recording || warming}>
        <span ref={labelRef} role="status" title={label}>
          {label}
        </span>
      </div>
      {processing && (
        <button
          className="ol-default-processing-cancel"
          aria-label={`${label} · ${t('common.cancel')}`}
          disabled={preview}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onCancel}
        />
      )}
    </motion.div>
  );
}

export function CapsuleStylePreview({ style }: { style: CapsuleStyle }) {
  return (
    <div className="ol-capsule-style-preview" data-style={style} aria-hidden="true">
      {style === 'siri' ? (
        <div className="ol-siri-capsule-preview">
          <VoiceOrbStage os="mac" state="recording" level={0.2} />
        </div>
      ) : (
        <OpenLessPill state="recording" level={0.2} preview />
      )}
    </div>
  );
}
