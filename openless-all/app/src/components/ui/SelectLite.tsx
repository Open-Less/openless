// SelectLite — 三平台统一的自定义下拉，替代 native <select>，避开 Win32 ComboBox
// 直角丑框（issue #418）以及 WKWebView 上原生 NSPopUpButton 的视觉割裂。
//
// 设计：
// - 触发器是 button（chevron + 当前值），样式可被 `style` 覆盖
// - popover 用 portal 渲染到 document.body，避开父容器 overflow:hidden
// - 键盘：ArrowDown/ArrowUp 切换高亮，Enter 确认，Esc 关闭
// - 点击外部 / 滚动外部容器都会关闭（popover 内部 scroll 不关闭）
// - 关闭有 .14s exit 动画
// - 二次定位走 popoverMounted state + useLayoutEffect 同步完成，整轮在 paint 前
//   收敛成最终 anchor，避免「兜底位置 paint 一次 + 真实位置再 paint 一次」的闪动
// - CSS `zoom` 补偿（关键）：fontScale.ts 通过 `html.style.zoom` 整体缩放页面。
//   WKWebView 在 zoom 下双标：getBoundingClientRect 返回 post-zoom（视觉）坐标，
//   而 position:fixed 的 left/top/width 被当 pre-zoom（布局）坐标处理。直接
//   `left=rect.left` 会让 popover 视觉位置 = rect.left × zoom，右偏
//   rect.left × (zoom-1) 像素。修法是 setAnchor 时把视觉坐标除以 zoom 转回布局
//   坐标，让浏览器渲染时 ×zoom 后回到正确视觉位置。详见 positionPopover。

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { Icon } from '../Icon';

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** 可选：渲染在选项标签右侧、勾选标记左侧（如麦克风音量条）。 */
  trailing?: ReactNode;
}

interface SelectLiteProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  placeholder?: string;
  disabled?: boolean;
  style?: CSSProperties;
  ariaLabel?: string;
  /** 下拉打开 / 关闭时回调 —— 让调用方按开合状态启停副作用（如电平监听）。 */
  onOpenChange?: (open: boolean) => void;
}

const DEFAULT_TRIGGER_STYLE: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 8,
  padding: '0 10px',
  height: 32,
  fontSize: 12.5,
  fontFamily: 'inherit',
  borderRadius: 8,
  border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-select-trigger-bg)',
  color: 'var(--ol-ink)',
  cursor: 'default',
  outline: 'none',
  textAlign: 'left',
  minWidth: 160,
};

const EXIT_ANIM_MS = 140;

export function SelectLite({
  value,
  onChange,
  options,
  placeholder,
  disabled = false,
  style,
  ariaLabel,
  onOpenChange,
}: SelectLiteProps) {
  const [open, setOpen] = useState(false);
  // leaving 让 popover 在卸载前播完 exit keyframe（用户报"没有收缩动画"——之前直接 unmount）
  const [leaving, setLeaving] = useState(false);
  const [highlight, setHighlight] = useState<number>(-1);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const [anchor, setAnchor] = useState<{ left: number; top: number; width: number } | null>(null);
  // popoverMounted 让 useLayoutEffect 在 popover 实际进入 DOM 后再触发一次 positionPopover，
  // 而且整轮发生在 paint 之前——见下方 useLayoutEffect 注释。
  const [popoverMounted, setPopoverMounted] = useState(false);

  const selected = useMemo(
    () => options.find(opt => opt.value === value),
    [options, value],
  );
  const displayLabel = selected?.label ?? placeholder ?? '';

  const positionPopover = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    // popover 高度只用真实测量；fallback 280 仅在首帧 popover 还没挂载时用。
    const popoverHeight = popoverRef.current?.getBoundingClientRect().height ?? 280;
    // 纵向：默认在触发器下方；若下方空间放不下 popover，翻转向上避免被视口裁剪。
    const spaceBelow = window.innerHeight - rect.bottom;
    const flipUp = spaceBelow < popoverHeight + 8 && rect.top > popoverHeight + 8;
    const visualTop = flipUp ? rect.top - popoverHeight - 4 : rect.bottom + 4;
    // popover 强制 width=trigger.width（见下方 style），所以 maxLeft 用 rect.width 算；
    // popover 没挂载和挂载后两帧 left 一致，避免 first-paint 跳位。
    const minLeft = 8;
    const maxLeft = Math.max(minLeft, window.innerWidth - rect.width - 8);
    const visualLeft = Math.min(Math.max(rect.left, minLeft), maxLeft);
    // ── CSS zoom 补偿（root cause of 位置偏移） ──
    // fontScale 通过 `document.documentElement.style.zoom` 整体缩放页面（见 fontScale.ts）。
    // WKWebView 在 zoom 下双标：getBoundingClientRect 返回 post-zoom（视觉）坐标，
    // 而 position:fixed 的 left/top/width 被当 pre-zoom（布局）坐标处理，渲染时再 × zoom。
    // 如果直接 set left=rect.left，popover 视觉会偏到 rect.left × zoom 处（右移 rect.left×(zoom-1)）。
    // 这里把视觉坐标除以 zoom 转回布局坐标，让 position:fixed 渲染回到正确视觉位置。
    const zoomStr = document.documentElement.style.zoom;
    const zoom = zoomStr ? parseFloat(zoomStr) || 1 : 1;
    setAnchor({
      left: visualLeft / zoom,
      top: visualTop / zoom,
      width: rect.width / zoom,
    });
  }, []);

  // popover ref callback：每次 popover DOM mount/unmount 调一次，只翻 popoverMounted。
  // 真正的二次定位由下方 useLayoutEffect 拿 popoverMounted 的依赖触发——这样第二次
  // positionPopover 同步发生在 paint 之前，而不是之前的 requestAnimationFrame（RAF 已
  // 在 paint 之后），避免「先用 280 高度兜底 paint 一次，再校正成真实位置 paint 一次」
  // 的双 paint 闪动（flipUp 决策在 280 fallback vs. 真实高度间可能反转）。
  const setPopoverRef = useCallback((node: HTMLDivElement | null) => {
    popoverRef.current = node;
    setPopoverMounted(!!node);
  }, []);

  // 两阶段定位都同步在 paint 前完成：
  // 1) open 由 false→true：popoverRef 还是 null，positionPopover 用 280 高度兜底设
  //    一次 anchor，让 portal 条件 `open && anchor` 通过、popover 进 DOM（避免 v1.3.1-8
  //    之前 anchor=null 永不渲染的死锁）。
  // 2) popoverMounted 由 false→true：popoverRef 已经指向真实 DOM，positionPopover
  //    用真实高度算出最终 anchor。整轮 commit→layoutEffect→re-commit→layoutEffect
  //    都在浏览器 paint 之前完成，用户只看到一帧最终位置，没有闪动。
  useLayoutEffect(() => {
    if (!open) return;
    positionPopover();
  }, [open, popoverMounted, positionPopover]);

  // 键盘 ArrowUp/Down 改 highlight 后把高亮项 scroll into view —— 长 dropdown 超过
  // maxHeight 280 时键盘用户能看到当前高亮。
  useEffect(() => {
    if (!open || highlight < 0) return;
    const target = popoverRef.current?.querySelector(
      `[data-option-index="${highlight}"]`,
    ) as HTMLElement | null;
    target?.scrollIntoView({ block: 'nearest' });
  }, [highlight, open]);

  // 点击外部 / 滚动外部 → 关闭。popover 内部 scroll 保持打开。
  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (triggerRef.current?.contains(target)) return;
      if (popoverRef.current?.contains(target)) return;
      closeMenu();
    };
    // 用户在 popover 外部任何位置滚动（wheel 或 scroll 事件）→ 关闭。
    // popover 内部滚动（长列表 scroll）popover.contains(target) → 保留打开。
    const handleScrollOutside = (event: Event) => {
      const target = event.target as Node | null;
      if (target && popoverRef.current?.contains(target)) return;
      closeMenu();
    };
    // window resize 强制关闭：重算位置成本高且大多数 resize 表明 user 不再想看 popover。
    const handleResize = () => closeMenu();

    document.addEventListener('mousedown', handlePointerDown);
    window.addEventListener('scroll', handleScrollOutside, { capture: true, passive: true });
    window.addEventListener('wheel', handleScrollOutside, { capture: true, passive: true });
    window.addEventListener('resize', handleResize);
    return () => {
      document.removeEventListener('mousedown', handlePointerDown);
      window.removeEventListener('scroll', handleScrollOutside, true);
      window.removeEventListener('wheel', handleScrollOutside, true);
      window.removeEventListener('resize', handleResize);
    };
    // closeMenu 是稳定引用（无 React state 依赖），不放 deps。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const openMenu = () => {
    if (disabled) return;
    const initial = options.findIndex(opt => opt.value === value && !opt.disabled);
    setHighlight(initial >= 0 ? initial : options.findIndex(opt => !opt.disabled));
    setLeaving(false);
    setOpen(true);
    onOpenChange?.(true);
  };

  const closeMenu = () => {
    if (!open) return;
    onOpenChange?.(false);
    setLeaving(true);
    window.setTimeout(() => {
      setOpen(false);
      setLeaving(false);
      setHighlight(-1);
      setAnchor(null);
    }, EXIT_ANIM_MS);
  };

  const selectIndex = (index: number) => {
    const option = options[index];
    if (!option || option.disabled) return;
    onChange(option.value);
    closeMenu();
    triggerRef.current?.focus();
  };

  const moveHighlight = (direction: 1 | -1) => {
    if (options.length === 0) return;
    let next = highlight;
    for (let i = 0; i < options.length; i += 1) {
      next = (next + direction + options.length) % options.length;
      if (!options[next]?.disabled) {
        setHighlight(next);
        return;
      }
    }
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;
    if (!open) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        openMenu();
      }
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      closeMenu();
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveHighlight(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveHighlight(-1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (highlight >= 0) selectIndex(highlight);
    } else if (event.key === 'Tab') {
      closeMenu();
    }
  };

  const triggerStyle: CSSProperties = {
    ...DEFAULT_TRIGGER_STYLE,
    ...style,
    opacity: disabled ? 0.5 : 1,
    cursor: disabled ? 'not-allowed' : 'default',
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="ol-focus-ring"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-disabled={disabled}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => (open ? closeMenu() : openMenu())}
        onKeyDown={handleKeyDown}
        style={triggerStyle}
      >
        <span
          style={{
            flex: 1,
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            color: selected ? 'var(--ol-ink)' : 'var(--ol-ink-4)',
          }}
        >
          {displayLabel}
        </span>
        <Icon name="chevDown" size={11} />
      </button>
      {open && anchor && createPortal(
        <div
          ref={setPopoverRef}
          role="listbox"
          style={{
            position: 'fixed',
            left: anchor.left,
            top: anchor.top,
            // 锁到 trigger 宽（不是 minWidth），避免 content 撑大让 popover 跑出
            // trigger 范围；长 label 走 textOverflow:ellipsis 截断。
            width: anchor.width,
            maxHeight: 280,
            overflowY: 'auto',
            padding: 4,
            borderRadius: 10,
            border: '0.5px solid var(--ol-select-popover-border)',
            background: 'var(--ol-select-popover-bg)',
            backdropFilter: 'blur(20px) saturate(180%)',
            WebkitBackdropFilter: 'blur(20px) saturate(180%)',
            boxShadow: 'var(--ol-select-popover-shadow)',
            zIndex: 9999,
            fontFamily: 'inherit',
            fontSize: 12.5,
            animation: leaving
              ? 'ol-select-pop-out .14s cubic-bezier(.4,.0,.7,.2) forwards'
              : 'ol-select-pop .14s var(--ol-motion-quick) both',
            transformOrigin: 'top center',
          }}
        >
          {options.map((option, index) => {
            const isSelected = option.value === value;
            const isHighlighted = index === highlight;
            return (
              <div
                key={option.value || `__opt_${index}`}
                data-option-index={index}
                role="option"
                aria-selected={isSelected}
                aria-disabled={option.disabled}
                onMouseEnter={() => !option.disabled && setHighlight(index)}
                onMouseDown={event => {
                  event.preventDefault();
                  selectIndex(index);
                }}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  padding: '7px 10px',
                  borderRadius: 6,
                  cursor: option.disabled ? 'not-allowed' : 'default',
                  opacity: option.disabled ? 0.45 : 1,
                  background: isHighlighted && !option.disabled
                    ? 'var(--ol-select-option-hover-bg)'
                    : 'transparent',
                  color: isSelected ? 'var(--ol-blue)' : 'var(--ol-ink)',
                  fontWeight: isSelected ? 600 : 500,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  transition: 'background 0.10s var(--ol-motion-quick)',
                }}
              >
                <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {option.label}
                </span>
                {option.trailing}
                {isSelected && <Icon name="check" size={12} />}
              </div>
            );
          })}
        </div>,
        document.body,
      )}
    </>
  );
}
