// ThinkingDots.tsx — six-dot "thinking" indicator, the app-wide thinking visual language.
//
// Same imagery as the capsule's WebGL fluid dots (SiriGL orb): six colored dots
// orbiting the center with a slight gather/scatter breathing. Panel/list indicators
// are small (16~28px) where the WebGL orb's detail is unreadable and each instance
// costs a GL context, so this is pure CSS — animating transform/opacity only
// (compositor-friendly), sharp at any size, visible on light backgrounds (solid dots).

import type { CSSProperties } from 'react';

interface ThinkingDotsProps {
  /** Circumscribed-circle diameter (px), default 20. */
  size?: number;
  style?: CSSProperties;
}

/** Six-dot palette: the low-saturation end of the Siri spectrum, readable in light and dark themes. */
const DOT_COLORS = ['#5b8def', '#8b6ff2', '#c96fd6', '#e08787', '#d9a75f', '#5fb8a8'];

export function ThinkingDots({ size = 20, style }: ThinkingDotsProps) {
  const dot = Math.max(2.5, size * 0.17);
  const radius = size * 0.31;
  return (
    <span
      aria-hidden
      style={{
        position: 'relative',
        display: 'inline-block',
        width: size,
        height: size,
        flexShrink: 0,
        animation:
          'ol-thinking-spin 1.7s linear infinite, ol-thinking-breathe 2.8s ease-in-out infinite',
        willChange: 'transform',
        ...style,
      }}
    >
      {DOT_COLORS.map((color, i) => (
        <span
          key={i}
          style={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            width: dot,
            height: dot,
            marginLeft: -dot / 2,
            marginTop: -dot / 2,
            borderRadius: 999,
            background: color,
            transform: `rotate(${i * 60}deg) translateY(${-radius}px)`,
          }}
        />
      ))}
    </span>
  );
}

const THINKING_DOTS_KEYFRAMES = `
@keyframes ol-thinking-spin {
  to { transform: rotate(360deg); }
}
@keyframes ol-thinking-breathe {
  0%, 100% { scale: 1; opacity: 1; }
  50%      { scale: .82; opacity: .8; }
}
@media (prefers-reduced-motion: reduce) {
  [style*="ol-thinking-spin"] { animation: none; }
}
`;

if (typeof document !== 'undefined' && !document.getElementById('ol-thinking-dots-style')) {
  const tag = document.createElement('style');
  tag.id = 'ol-thinking-dots-style';
  tag.textContent = THINKING_DOTS_KEYFRAMES;
  document.head.appendChild(tag);
}
