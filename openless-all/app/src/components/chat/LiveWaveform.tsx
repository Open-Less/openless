import { useEffect, useRef } from 'react';

const BAR_WIDTH = 3;
const BAR_GAP = 3;
const SAMPLE_MS = 70;
const MIN_BAR = 2;

function clampLevel(level: number): number {
  return Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
}

/**
 * Scrolling history of the real microphone level. Each bar is one sampled
 * level (fast attack, slow release); nothing is synthesized while recording.
 * `processing` replaces the history with a quiet travelling pulse.
 */
export function LiveWaveform({
  level,
  processing = false,
  label,
}: {
  level: number;
  processing?: boolean;
  label: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const levelRef = useRef(0);
  const processingRef = useRef(processing);
  levelRef.current = clampLevel(level);
  processingRef.current = processing;

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) return;
    const reducedMotion =
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    let history: number[] = [];
    let smoothed = 0;
    let lastSample = performance.now();
    let width = 0;
    let height = 0;
    let color = '';
    let colorAge = 0;
    let frame = 0;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const ratio = window.devicePixelRatio || 1;
      width = rect.width;
      height = rect.height;
      canvas.width = Math.max(1, Math.round(width * ratio));
      canvas.height = Math.max(1, Math.round(height * ratio));
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
    };
    resize();
    const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(resize) : null;
    observer?.observe(canvas);

    const bar = (x: number, amplitude: number, alpha: number) => {
      const h = Math.max(MIN_BAR, amplitude * height * 0.92);
      const y = (height - h) / 2;
      context.globalAlpha = alpha;
      context.beginPath();
      if (typeof context.roundRect === 'function')
        context.roundRect(x, y, BAR_WIDTH, h, BAR_WIDTH / 2);
      else context.rect(x, y, BAR_WIDTH, h);
      context.fill();
    };

    const draw = (now: number) => {
      const step = BAR_WIDTH + BAR_GAP;
      const capacity = Math.max(1, Math.ceil(width / step) + 1);
      if (colorAge-- <= 0) {
        color = getComputedStyle(canvas).color;
        colorAge = 30;
      }
      context.clearRect(0, 0, width, height);
      context.fillStyle = color;

      if (processingRef.current) {
        smoothed = 0;
        history = [];
        for (let j = 0; j < capacity; j += 1) {
          const wave = reducedMotion ? 0.5 : 0.5 + 0.5 * Math.sin(now / 240 - j * 0.45);
          bar(width - BAR_WIDTH - j * step, 0.1 + 0.16 * wave, 0.35 + 0.35 * wave);
        }
      } else {
        const target = levelRef.current;
        smoothed += (target - smoothed) * (target > smoothed ? 0.6 : 0.2);
        if (now - lastSample >= SAMPLE_MS) {
          history.push(smoothed);
          lastSample = now;
          if (history.length > capacity) history = history.slice(-capacity);
        }
        const drift = reducedMotion ? 0 : Math.min(1, (now - lastSample) / SAMPLE_MS);
        for (let j = 0; j < capacity; j += 1) {
          const sample = history[history.length - 1 - j] ?? 0;
          const amplitude = Math.min(1, Math.sqrt(sample) * 1.08);
          const age = j / capacity;
          bar(width - BAR_WIDTH - (j + drift) * step, amplitude, 1 - age * 0.65);
        }
      }
      context.globalAlpha = 1;
      frame = window.requestAnimationFrame(draw);
    };
    frame = window.requestAnimationFrame(draw);
    return () => {
      window.cancelAnimationFrame(frame);
      observer?.disconnect();
    };
  }, []);

  return (
    <div
      className={`lc-live-waveform${processing ? ' is-processing' : ''}`}
      role="status"
      aria-label={label}
    >
      <canvas ref={canvasRef} aria-hidden="true" />
    </div>
  );
}
