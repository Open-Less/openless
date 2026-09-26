// Modal — 居中弹窗：backdrop + 卡片。风格市场详情 / 上传 / 我的发布 / GitHub 登录
// 等共用同一套弹出逻辑，避免每处各写一个。
//
// 动画在 overlays.css 的 .ol-dialog-overlay / .ol-dialog-card 上（纯 opacity +
// transform，不碰 blur）。退场使用独立的 *-out 关键帧：仅反转同名动画的方向不会
// 重新播放，已结束的入场动画会直接跳到起始帧，表现为「啪」地消失。

import { useEffect, useRef, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

interface ModalProps {
  children: ReactNode;
  onClose: () => void;
  /** 默认 50；多层叠加时（如登录弹窗叠在「我的发布」之上）传更大的值。 */
  zIndex?: number;
  /** 卡片宽度，默认 'min(560px, 100%)'。 */
  width?: string;
  /** 需要固定标题和底栏的弹窗可由内部内容区负责滚动。 */
  style?: CSSProperties;
  /** true 时播放退场动画；调用方用
   *  useExitMount 门控卸载时机，动画播完再 unmount。 */
  closing?: boolean;
  /** 宿主窗口定制遮罩与卡片外观（例如圆角浮窗需要圆角遮罩）。 */
  overlayClassName?: string;
  labelledBy?: string;
}

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), select:not([disabled]), input:not([disabled]):not([type="hidden"]), [tabindex]:not([tabindex="-1"])';

// Only the top-most dialog keeps Tab cycling inside itself.
const openModals: symbol[] = [];

function focusableWithin(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) => element.getClientRects().length > 0,
  );
}

export function Modal({
  children,
  onClose,
  zIndex = 50,
  width = 'min(560px, 100%)',
  style,
  closing = false,
  overlayClassName,
  labelledBy,
}: ModalProps) {
  const cardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const id = Symbol('modal');
    openModals.push(id);
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const card = cardRef.current;
    if (card && !card.contains(document.activeElement)) card.focus({ preventScroll: true });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Tab' || !card || openModals[openModals.length - 1] !== id) return;
      const items = focusableWithin(card);
      const active = document.activeElement;
      if (items.length === 0) {
        event.preventDefault();
        card.focus({ preventScroll: true });
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey) {
        if (active === first || active === card || !card.contains(active)) {
          event.preventDefault();
          last.focus();
        }
      } else if (active === last || !card.contains(active)) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      const index = openModals.indexOf(id);
      if (index >= 0) openModals.splice(index, 1);
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, []);

  // Portal 到 document.body：弹窗常从设置 / 市场等面板内部触发，而窗口 chrome
  // （WindowChrome）和页面容器带常驻 `will-change: transform`，会创建 containing
  // block —— 直接渲染的话 backdrop 的 `position: fixed` 会相对那个祖先而非视口定位，
  // 遮罩盖不住整窗（只压暗触发它的那块面板，比如 GitHub 登录浮在亮着的设置页上）。
  // Portal 出去后 fixed 相对视口，遮罩铺满全局。与 Tooltip / SelectLite 同款做法。
  return createPortal(
    <div
      className={`ol-dialog-overlay${closing ? ' is-closing' : ''}${overlayClassName ? ` ${overlayClassName}` : ''}`}
      onClick={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'var(--ol-dialog-backdrop)',
        display: 'grid',
        placeItems: 'center',
        zIndex,
        padding: 20,
        pointerEvents: closing ? 'none' : undefined,
      }}
    >
      <div
        ref={cardRef}
        className="ol-dialog-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        style={{
          width,
          maxHeight: '85vh',
          overflow: 'auto',
          borderRadius: 'var(--ol-dialog-radius)',
          background: 'var(--ol-surface)',
          border: '1px solid var(--ol-dialog-border)',
          boxShadow: 'var(--ol-dialog-shadow)',
          padding: 24,
          outline: 'none',
          ...style,
        }}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
