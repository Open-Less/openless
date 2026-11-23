import { motion } from 'framer-motion';
import { memo, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { VoiceOrbStage } from './VoiceOrbStage';
import { useReducedMotion } from '../../lib/motion';
import type { OS } from '../../components/WindowChrome';
import type { CapsuleState, CapsuleStyle } from '../../lib/types';
import './CapsuleStyles.css';
import './OpenLessCapsule.css';

// Measured from the 60 fps reference: 232×64 physical pixels at 2×.
const ENVELOPE = [0.22, 0.34, 0.48, 0.65, 0.82, 1, 0.82, 0.65, 0.48, 0.34, 0.22];
const MORPH = { duration: 0.22, ease: [0.22, 1, 0.36, 1] as const };
export const DEFAULT_CAPSULE_EXIT_MS = 240;
/** Duration of the rightward water-wave fill before revealing the slow-connection marquee. */
const PROCESSING_WAVE_DURATION_MS = 4200;

const WAVE_BUBBLES = [
  { left: '18%', top: '58%', size: 3.5, delay: '0s', duration: '2.8s' },
  { left: '42%', top: '34%', size: 2.5, delay: '0.6s', duration: '3.1s' },
  { left: '64%', top: '62%', size: 3, delay: '1.1s', duration: '2.6s' },
  { left: '82%', top: '44%', size: 2.5, delay: '0.3s', duration: '2.9s' },
] as const;

const CapsuleWaterWave = memo(function CapsuleWaterWave() {
  // Single unified path for body + gentle sinusoidal right crest: guarantees uniform
  // translucency with zero alpha-overlap seam, looping seamlessly over a 32px period.
  const waveCurve =
    'M 124 0 C 127.5 5.3, 127.5 10.7, 124 16 C 120.5 21.3, 120.5 26.7, 124 32 C 127.5 37.3, 127.5 42.7, 124 48 C 120.5 53.3, 120.5 58.7, 124 64';
  const fillPath = `M 0 0 L 124 0 C 127.5 5.3, 127.5 10.7, 124 16 C 120.5 21.3, 120.5 26.7, 124 32 C 127.5 37.3, 127.5 42.7, 124 48 C 120.5 53.3, 120.5 58.7, 124 64 L 0 64 Z`;
  return (
    <div className="ol-capsule-wave-layer" aria-hidden="true">
      <div className="ol-capsule-wave-progress">
        <svg
          className="ol-capsule-wave-svg"
          viewBox="0 0 132 64"
          preserveAspectRatio="none"
        >
          <path d={fillPath} fill="var(--ol-pill-wave-fill)" />
          <path
            d={waveCurve}
            fill="none"
            stroke="var(--ol-pill-wave-edge)"
            strokeWidth="1.15"
            strokeLinecap="round"
          />
        </svg>
        <div className="ol-capsule-wave-bubbles">
          {WAVE_BUBBLES.map((b, i) => (
            <span
              key={i}
              className="ol-capsule-wave-bubble"
              style={{
                left: b.left,
                top: b.top,
                width: b.size,
                height: b.size,
                animationDelay: b.delay,
                animationDuration: b.duration,
              }}
            />
          ))}
        </div>
      </div>
    </div>
  );
});

const CapsuleProcessingLens = memo(function CapsuleProcessingLens({
  label,
}: {
  label: string;
}) {
  const chars = Array.from(label);
  const lastIndex = Math.max(1, chars.length - 1);
  return (
    <span className="ol-lens-word" role="status" aria-label={label} title={label}>
      {chars.map((char, index) => {
        // Non-linear ease-out stagger (fast at the start, slower toward the end)
        // matches the water wave's deceleration so each glyph peaks right on the crest.
        const progress = index / lastIndex;
        const delayMs = Math.round(140 + Math.pow(progress, 1.65) * 1650);
        return (
          <span
            key={`${index}-${char}`}
            className="ol-lens-char"
            aria-hidden="true"
            style={{ animationDelay: `${delayMs}ms` }}
          >
            {char === ' ' ? '\u00A0' : char}
          </span>
        );
      })}
    </span>
  );
});

function useCapsulePresence(ref: RefObject<HTMLDivElement>, leaving: boolean, preview: boolean) {
  const reduced = useReducedMotion();
  const interrupted = useRef<Keyframe | null>(null);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    // GPU-composited opacity + translate3d only (avoid animating filter: blur alongside
    // backdrop-filter, which causes software repaints and flicker on Windows WebView2).
    const visible = { opacity: 1, transform: 'translate3d(0, 0px, 0) scale(1)' };
    const hidden = {
      opacity: 0,
      transform: 'translate3d(0, 44px, 0) scale(0.94)',
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
    node.style.willChange = 'opacity, transform';
    const animation = node.animate([from, target], {
      duration: leaving ? DEFAULT_CAPSULE_EXIT_MS : 320,
      easing: leaving ? 'cubic-bezier(0.4, 0, 0.85, 0.6)' : 'cubic-bezier(0.22, 1, 0.36, 1)',
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
        // GPU transform scaleY avoids 60fps per-bar layout reflow on macOS and Windows.
        const ripple = reduced ? 1 : 0.86 + 0.14 * Math.sin(now / 85 + index * 0.8);
        const heightPx = 1.5 + 14.5 * Math.pow(smoothed, 0.6) * envelope * ripple;
        (bars[index] as HTMLElement).style.transform = `scaleY(${(heightPx / 16).toFixed(3)})`;
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
  const [slowConnection, setSlowConnection] = useState(false);
  useEffect(() => {
    if (!processing || preview) {
      setSlowConnection(false);
      return;
    }
    setSlowConnection(false);
    const timer = window.setTimeout(() => {
      setSlowConnection(true);
    }, PROCESSING_WAVE_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [processing, preview]);

  const slowLabel = t('capsule.slowConnection');
  const label = processing
    ? slowConnection
      ? slowLabel
      : translation
        ? t('capsule.translating')
        : message || t(operating ? 'capsule.using' : 'capsule.thinking')
    : state === 'done'
      ? message || t('capsule.inserted', { count: insertedChars })
      : state === 'cancelled'
        ? t('capsule.cancelled')
        : message || t('capsule.error');
  const labelRef = useRef<HTMLSpanElement>(null);
  const [statusWidth, setStatusWidth] = useState(116);
  useLayoutEffect(() => {
    // Only error states expand beyond 116px; skip ResizeObserver during 60Hz recording/processing.
    if (state !== 'error') return;
    const element = labelRef.current;
    if (!element) return;
    const measure = () =>
      setStatusWidth(
        Math.min(
          label === t('capsule.emptyTranscript') ? 444 : 300,
          Math.max(116, element.scrollWidth + 28),
        ),
      );
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [label, state, t]);
  // Keep capsule width constant at 116px during recording, processing, done, and cancelled;
  // processing copy is clipped inside the capsule instead of expanding it.
  const targetWidth =
    leaving || warming ? 68 : state === 'error' ? statusWidth : 116;
  return (
    <motion.div
      ref={root}
      className="ol-capsule-pill ol-default-capsule"
      data-recording={recording}
      data-warming={warming}
      data-processing={processing}
      data-slow-connection={processing && slowConnection}
      data-translation={translation && (recording || warming)}
      data-error={state === 'error'}
      data-leaving={leaving}
      initial={preview || reduced ? false : { width: 68 }}
      animate={{
        width: targetWidth,
      }}
      transition={reduced ? { duration: 0 } : MORPH}
    >
      {processing && !preview && <CapsuleWaterWave />}
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
        {processing && slowConnection ? (
          <div className="ol-capsule-marquee" role="status" title={slowLabel}>
            <div className="ol-capsule-marquee-track">
              <span className="ol-capsule-marquee-item">{slowLabel}</span>
              <span className="ol-capsule-marquee-item" aria-hidden="true">
                {slowLabel}
              </span>
            </div>
          </div>
        ) : processing && !preview ? (
          <CapsuleProcessingLens label={label} />
        ) : (
          <span ref={labelRef} role="status" title={label}>
            {label}
          </span>
        )}
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
