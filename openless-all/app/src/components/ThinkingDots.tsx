// ThinkingDots.tsx — 六圆点「思考中」指示，全应用统一的思考视觉语言。
//
// 与胶囊的 WebGL 流体圆点（SiriGL orb）同一意象：六个彩点绕圆心转动 + 轻微
// 聚散呼吸。面板/列表里的指示尺寸小（16~28px），WebGL orb 在这个尺寸下细节
// 不可辨且每个实例占一个 GL context，所以这里用纯 CSS 实现 —— 只动
// transform/opacity（合成器友好），任意尺寸清晰，浅色底上依然可见（实色点）。

import type { CSSProperties } from 'react';

interface ThinkingDotsProps {
  /** 外接圆直径（px），默认 20。 */
  size?: number;
  style?: CSSProperties;
}

/** 六点色板：取 Siri 光谱的低饱和段，深浅主题下都可读。 */
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
        animation: 'ol-thinking-spin 1.7s linear infinite, ol-thinking-breathe 2.8s ease-in-out infinite',
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
