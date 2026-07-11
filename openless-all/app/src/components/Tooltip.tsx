// Tooltip.tsx — 统一的悬浮提示，macOS 菜单式「预热」行为：
//
//   - 首次 hover：延迟 600ms 才出现 —— 日常操作扫过按钮不触发一堆提示；
//   - 预热窗口内（某个 tooltip 正显示、或刚关闭 <300ms）再 hover 相邻目标：
//     0 延迟即时切换 —— 想连续查看各菜单功能说明时不用每个都等。
//
// 预热状态是模块级共享的，所有 Tooltip 实例天然联动。渲染用 portal + fixed
// 定位，不受祖先 overflow / transform / backdrop-filter 包含块影响。

import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

/** 首次 hover 到出现的延迟。 */
const WARM_DELAY_MS = 600;
/** tooltip 关闭后预热状态的存续窗口：这段时间内 hover 相邻目标即时显示。 */
const WARM_LINGER_MS = 300;

let warmUntil = 0;

interface TooltipProps {
  content: ReactNode;
  /** 提示出现在锚点的哪一侧，默认 right（适合左侧栏菜单）。 */
  placement?: 'right' | 'top' | 'bottom';
  children: ReactNode;
}

export function Tooltip({ content, placement = 'right', children }: TooltipProps) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const timerRef = useRef<number | null>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  const show = () => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    if (placement === 'right') {
      setPos({ x: rect.right + 10, y: rect.top + rect.height / 2 });
    } else if (placement === 'top') {
      setPos({ x: rect.left + rect.width / 2, y: rect.top - 8 });
    } else {
      setPos({ x: rect.left + rect.width / 2, y: rect.bottom + 8 });
    }
  };

  const hide = () => {
    if (timerRef.current != null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setPos(prev => {
      if (prev) warmUntil = Date.now() + WARM_LINGER_MS;
      return null;
    });
  };

  const onEnter = () => {
    if (Date.now() < warmUntil) {
      show();
      return;
    }
    timerRef.current = window.setTimeout(show, WARM_DELAY_MS);
  };

  useEffect(
    () => () => {
      if (timerRef.current != null) window.clearTimeout(timerRef.current);
    },
    [],
  );

  return (
    // display:grid 单格包装：span 尺寸与子元素一致（display:contents 的 rect 恒为 0
    // 无法定位），又不给 flex/grid 父容器引入额外的布局盒差异。
    <span
      ref={anchorRef}
      onMouseEnter={onEnter}
      onMouseLeave={hide}
      onMouseDown={hide}
      style={{ display: 'grid' }}
    >
      {children}
      {pos != null &&
        createPortal(
          <span role="tooltip" style={{ ...bubbleStyle, ...placementStyle(placement, pos) }}>
            {content}
          </span>,
          document.body,
        )}
    </span>
  );
}

function placementStyle(placement: 'right' | 'top' | 'bottom', pos: { x: number; y: number }): CSSProperties {
  if (placement === 'right') {
    return { left: pos.x, top: pos.y, transform: 'translateY(-50%)' };
  }
  if (placement === 'top') {
    return { left: pos.x, top: pos.y, transform: 'translate(-50%, -100%)' };
  }
  return { left: pos.x, top: pos.y, transform: 'translateX(-50%)' };
}

const bubbleStyle: CSSProperties = {
  position: 'fixed',
  zIndex: 1000,
  maxWidth: 260,
  padding: '5px 9px',
  borderRadius: 8,
  fontSize: 11.5,
  fontWeight: 500,
  lineHeight: 1.45,
  color: 'var(--ol-ink)',
  background: 'var(--ol-glass-bg-strong)',
  backdropFilter: 'blur(12px) saturate(150%)',
  WebkitBackdropFilter: 'blur(12px) saturate(150%)',
  border: '0.5px solid var(--ol-glass-border)',
  boxShadow: 'var(--ol-shadow-md)',
  whiteSpace: 'nowrap',
  pointerEvents: 'none',
  animation: 'ol-tooltip-in 0.12s ease-out both',
};

const TOOLTIP_KEYFRAMES = `
@keyframes ol-tooltip-in {
  from { opacity: 0; scale: .96; }
  to   { opacity: 1; scale: 1; }
}
`;

if (typeof document !== 'undefined' && !document.getElementById('ol-tooltip-style')) {
  const tag = document.createElement('style');
  tag.id = 'ol-tooltip-style';
  tag.textContent = TOOLTIP_KEYFRAMES;
  document.head.appendChild(tag);
}
