/**
 * 统一的悬浮提示，支持 hover、键盘 focus 和触摸触发。
 *
 * Tooltip 使用 portal + fixed 定位，不受父级 overflow、transform 或 backdrop-filter 影响。
 */
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';

/** 首次 hover 到出现的延迟。 */
const WARM_DELAY_MS = 600;
/** tooltip 关闭后的预热窗口。 */
const WARM_LINGER_MS = 300;

let warmUntil = 0;

interface TooltipProps {
  content: ReactNode;
  /** 提示出现在锚点的哪一侧，默认 right。 */
  placement?: 'right' | 'top' | 'bottom';
  /** 整句说明允许换行，默认 nowrap。 */
  wrap?: boolean;
  /** 让标签本身成为可聚焦的 Tooltip 触发器。 */
  focusable?: boolean;
  children: ReactNode;
}

type TooltipPlacement = NonNullable<TooltipProps['placement']>;

interface AnchorRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
  width: number;
  height: number;
}

interface TooltipPosition {
  anchor: AnchorRect;
  placement: TooltipPlacement;
  offsetX: number;
  offsetY: number;
}

export function Tooltip({
  content,
  placement = 'right',
  wrap = false,
  focusable = false,
  children,
}: TooltipProps) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const bubbleRef = useRef<HTMLSpanElement>(null);
  const timerRef = useRef<number | null>(null);
  const hoveredRef = useRef(false);
  const focusedRef = useRef(false);
  const tooltipId = useId();
  const [pos, setPos] = useState<TooltipPosition | null>(null);

  const readAnchorRect = (): AnchorRect | null => {
    const anchor = anchorRef.current;
    if (!anchor) return null;
    const rect = anchor.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      top: rect.top,
      bottom: rect.bottom,
      width: rect.width,
      height: rect.height,
    };
  };

  const show = () => {
    const anchor = readAnchorRect();
    if (!anchor) return;
    setPos({ anchor, placement, offsetX: 0, offsetY: 0 });
  };

  const clearTimer = () => {
    if (timerRef.current == null) return;
    window.clearTimeout(timerRef.current);
    timerRef.current = null;
  };

  const scheduleShow = () => {
    clearTimer();
    if (Date.now() < warmUntil) {
      show();
      return;
    }
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      show();
    }, WARM_DELAY_MS);
  };

  const hide = () => {
    clearTimer();
    setPos(prev => {
      if (prev) warmUntil = Date.now() + WARM_LINGER_MS;
      return null;
    });
  };

  const hideIfInactive = () => {
    if (!hoveredRef.current && !focusedRef.current) hide();
  };

  const onEnter = () => {
    hoveredRef.current = true;
    scheduleShow();
  };

  const onLeave = () => {
    hoveredRef.current = false;
    hideIfInactive();
  };

  const onFocus = () => {
    focusedRef.current = true;
    clearTimer();
    show();
  };

  const onBlur = () => {
    focusedRef.current = false;
    hideIfInactive();
  };

  const onPointerDown = (event: PointerEvent<HTMLSpanElement>) => {
    if (event.pointerType === 'touch') {
      clearTimer();
      show();
      return;
    }
    hide();
  };

  useLayoutEffect(() => {
    const bubble = bubbleRef.current;
    if (!bubble || !pos) return;

    const margin = 8;
    const bubbleRect = bubble.getBoundingClientRect();
    let nextPlacement = pos.placement;

    if (
      pos.placement === 'bottom'
      && bubbleRect.bottom > window.innerHeight - margin
      && pos.anchor.top - 8 - bubbleRect.height >= margin
    ) {
      nextPlacement = 'top';
    } else if (
      pos.placement === 'top'
      && bubbleRect.top < margin
      && pos.anchor.bottom + 8 + bubbleRect.height <= window.innerHeight - margin
    ) {
      nextPlacement = 'bottom';
    }

    if (nextPlacement !== pos.placement) {
      setPos({ ...pos, placement: nextPlacement, offsetX: 0, offsetY: 0 });
      return;
    }

    const offsetX = Math.max(margin - bubbleRect.left, 0)
      - Math.max(bubbleRect.right - (window.innerWidth - margin), 0);
    const offsetY = Math.max(margin - bubbleRect.top, 0)
      - Math.max(bubbleRect.bottom - (window.innerHeight - margin), 0);

    if (Math.abs(offsetX) > 0.5 || Math.abs(offsetY) > 0.5) {
      setPos({
        ...pos,
        offsetX: pos.offsetX + offsetX,
        offsetY: pos.offsetY + offsetY,
      });
    }
  }, [pos]);

  useEffect(
    () => () => {
      clearTimer();
    },
    [],
  );

  return (
    // display:grid 保持锚点尺寸与子元素一致，同时避免引入额外布局差异。
    <span
      ref={anchorRef}
      onMouseEnter={onEnter}
      onMouseLeave={onLeave}
      onFocus={onFocus}
      onBlur={onBlur}
      onPointerDown={onPointerDown}
      tabIndex={focusable ? 0 : undefined}
      aria-describedby={focusable && pos != null ? tooltipId : undefined}
      style={{ display: 'grid' }}
    >
      {children}
      {pos != null &&
        createPortal(
          <span
            ref={bubbleRef}
            id={tooltipId}
            role="tooltip"
            style={{
              ...bubbleStyle,
              ...(wrap ? wrapBubbleStyle : null),
              ...placementStyle(pos),
            }}
          >
            {content}
          </span>,
          document.body,
        )}
    </span>
  );
}

function placementStyle(pos: TooltipPosition): CSSProperties {
  const { anchor, placement, offsetX, offsetY } = pos;
  if (placement === 'right') {
    return {
      left: anchor.right + 10 + offsetX,
      top: anchor.top + anchor.height / 2 + offsetY,
      transform: 'translateY(-50%)',
    };
  }
  if (placement === 'top') {
    return {
      left: anchor.left + anchor.width / 2 + offsetX,
      top: anchor.top - 8 + offsetY,
      transform: 'translate(-50%, -100%)',
    };
  }
  return {
    left: anchor.left + anchor.width / 2 + offsetX,
    top: anchor.bottom + 8 + offsetY,
    transform: 'translateX(-50%)',
  };
}

const bubbleStyle: CSSProperties = {
  position: 'fixed',
  zIndex: 1000,
  maxWidth: 'min(260px, calc(100vw - 16px))',
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

const wrapBubbleStyle: CSSProperties = {
  whiteSpace: 'normal',
  width: 'max-content',
  maxWidth: 'min(280px, calc(100vw - 16px))',
  maxHeight: 'calc(100vh - 16px)',
  overflowY: 'auto',
};

const TOOLTIP_KEYFRAMES = [
  '@keyframes ol-tooltip-in {',
  '  from { opacity: 0; scale: .96; }',
  '  to   { opacity: 1; scale: 1; }',
  '}',
].join('\n');

if (typeof document !== 'undefined' && !document.getElementById('ol-tooltip-style')) {
  const tag = document.createElement('style');
  tag.id = 'ol-tooltip-style';
  tag.textContent = TOOLTIP_KEYFRAMES;
  document.head.appendChild(tag);
}
